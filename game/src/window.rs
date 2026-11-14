use std::sync::Arc;

use femtovg::Canvas;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use wgpu::TextureUsages;
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    monitor::MonitorHandle,
};

use crate::config::GameConfig;

pub fn find_monitor(
    mut monitors: impl Iterator<Item = MonitorHandle>,
    pos: PhysicalPosition<i32>,
) -> Option<MonitorHandle> {
    monitors.find(|x| x.position() == pos)
}

type WindowCreation = (
    Arc<winit::window::Window>,
    wgpu::Surface<'static>,
    egui_wgpu::RenderState,
    Canvas<femtovg::renderer::WGPURenderer>,
    wgpu::SurfaceConfiguration,
);

pub fn present_mode(vsync: bool) -> wgpu::PresentMode {
    if vsync {
        wgpu::PresentMode::AutoVsync
    } else {
        wgpu::PresentMode::AutoNoVsync
    }
}

/// Mostly borrowed code from femtovg/examples
pub async fn create_window(el: &ActiveEventLoop) -> anyhow::Result<WindowCreation> {
    let settings = &GameConfig::get().graphics;

    let window_builder = Window::default_attributes()
        .with_resizable(true)
        .with_title("USC Game");

    #[cfg(not(target_os = "android"))]
    let window_builder = match settings.fullscreen {
        crate::config::Fullscreen::Windowed { pos, size } => {
            window_builder.with_position(pos).with_inner_size(size)
        }
        crate::config::Fullscreen::Borderless { monitor } => window_builder.with_fullscreen(Some(
            winit::window::Fullscreen::Borderless(find_monitor(el.available_monitors(), monitor)),
        )),
        crate::config::Fullscreen::Exclusive {
            monitor,
            resolution,
        } => {
            if let Some(mode) = find_monitor(el.available_monitors(), monitor).and_then(|monitor| {
                monitor
                    .video_modes()
                    .filter(|x| x.size() == resolution)
                    .max_by_key(|x| x.refresh_rate_millihertz())
            }) {
                window_builder.with_fullscreen(Some(winit::window::Fullscreen::Exclusive(mode)))
            } else {
                window_builder
            }
        }
    };

    let instance = wgpu::Instance::new(
        wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(
            el.owned_display_handle(),
        )),
    );

    let window = Arc::new(el.create_window(window_builder)?);

    let PhysicalSize { width, height } = window.inner_size();

    let surface = instance.create_surface(window.clone())?;

    let adapter = wgpu::util::initialize_adapter_from_env_or_default(&instance, Some(&surface))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to find an appropriate graphics adapter: {e}"))?;

    log::info!("Using graphics adapter: {:?}", adapter.get_info());

    // Create the logical device and command queue
    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                // Use what the adapter supports, skin shaders can use many uniforms and textures
                required_limits: adapter.limits(),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            },
        )
        .await?;

    let mut surface_config = surface
        .get_default_config(&adapter, width.max(1), height.max(1))
        .ok_or_else(|| anyhow::anyhow!("Surface not supported by adapter"))?;

    let swapchain_capabilities = surface.get_capabilities(&adapter);
    let swapchain_format = swapchain_capabilities
        .formats
        .iter()
        .find(|f| !f.is_srgb())
        .copied()
        .unwrap_or_else(|| swapchain_capabilities.formats[0]);
    surface_config.format = swapchain_format;
    surface_config.usage |= TextureUsages::COPY_SRC | TextureUsages::COPY_DST;
    surface_config.present_mode = present_mode(settings.vsync);
    surface.configure(&device, &surface_config);

    let renderer = egui_wgpu::Renderer::new(
        &device,
        surface_config.format,
        egui_wgpu::RendererOptions::default(),
    );

    let egui_state = egui_wgpu::RenderState {
        instance,
        adapter,
        available_adapters: vec![],
        device,
        queue,
        target_format: surface_config.format,
        renderer: Arc::new(egui::mutex::RwLock::new(renderer)),
    };

    let renderer =
        femtovg::renderer::WGPURenderer::new(egui_state.device.clone(), egui_state.queue.clone());

    let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
    canvas.set_size(width, height, window.scale_factor() as f32);

    Ok((window, surface, egui_state, canvas, surface_config))
}

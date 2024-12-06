use std::sync::Arc;

use femtovg::{Canvas, Renderer};
use winit::window::Window;
use winit::{event_loop::ActiveEventLoop, platform::x11::WindowAttributesExtX11};

use wgpu::{InstanceDescriptor, TextureUsages};
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

type WindowCreation<'a> = (
    Arc<winit::window::Window>,
    wgpu::Surface<'a>,
    egui_wgpu::RenderState,
    Canvas<femtovg::renderer::WGPURenderer>,
    wgpu::SurfaceConfiguration,
);

/// Mostly borrowed code from femtovg/examples
pub async fn create_window<'a>(el: &ActiveEventLoop) -> anyhow::Result<WindowCreation<'a>> {
    let settings = &GameConfig::get().graphics;

    let window_builder = Window::default_attributes()
        .with_resizable(true)
        .with_title("USC Game");

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

    let backends = wgpu::util::backend_bits_from_env().unwrap_or_default();
    let dx12_shader_compiler = wgpu::util::dx12_shader_compiler_from_env().unwrap_or_default();
    let gles_minor_version = wgpu::util::gles_minor_version_from_env().unwrap_or_default();

    let instance = wgpu::Instance::new(InstanceDescriptor {
        backends,
        flags: wgpu::InstanceFlags::from_build_config().with_env(),
        dx12_shader_compiler,
        gles_minor_version,
    });

    let window = Arc::new(el.create_window(window_builder)?);

    let PhysicalSize { width, height } = window.inner_size();

    let surface = instance.create_surface(window.clone())?;

    let adapter = wgpu::util::initialize_adapter_from_env_or_default(&instance, Some(&surface))
        .await
        .expect("Failed to find an appropriate adapter");

    // Create the logical device and command queue
    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                // Make sure we use the texture resolution limits from the adapter, so we can support images the size of the swapchain.
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
            },
            None,
        )
        .await
        .expect("Failed to create device");

    let mut surface_config = surface.get_default_config(&adapter, 1280, 720).unwrap();

    let swapchain_capabilities = surface.get_capabilities(&adapter);
    let swapchain_format = swapchain_capabilities
        .formats
        .iter()
        .find(|f| !f.is_srgb())
        .copied()
        .unwrap_or_else(|| swapchain_capabilities.formats[0]);
    surface_config.format = swapchain_format;
    surface_config.usage = surface_config.usage | TextureUsages::COPY_SRC | TextureUsages::COPY_DST;
    surface.configure(&device, &surface_config);

    let renderer =
        egui_wgpu::Renderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, None, 4, false);

    let device = Arc::new(device);

    let egui_state = egui_wgpu::RenderState {
        adapter: Arc::new(adapter),
        available_adapters: Arc::new([]),
        device: device,
        queue: Arc::new(queue),
        target_format: wgpu::TextureFormat::Rgba8Unorm,
        renderer: Arc::new(egui::mutex::RwLock::new(renderer)),
    };

    let renderer =
        femtovg::renderer::WGPURenderer::new(egui_state.device.clone(), egui_state.queue.clone());

    let mut canvas = Canvas::new(renderer).expect("Cannot create canvas");
    canvas.set_size(width, height, window.scale_factor() as f32);

    Ok((window, surface, egui_state, canvas, surface_config))
}

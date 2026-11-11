use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, SystemTime},
};

use anyhow::anyhow;
use di::{transient_factory, ServiceCollection};
use femtovg::rgb::{ComponentBytes, ComponentSlice};
use itertools::Itertools;
use puffin::profile_function;
#[cfg(not(target_os = "android"))]
use rfd::AsyncFileDialog;
use wgpu::{util::DeviceExt, Extent3d, Origin3d, SurfaceTexture, Texture, TextureUsages};
use winit::{dpi::PhysicalSize, event::ElementState};

use crate::{
    button_codes::{UscButton, UscInputEvent},
    config::GameConfig,
    vg_ui::Vgfx,
    worker_service::WorkerService,
    Viewport,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub command: bool,
}

pub async fn await_task<T: Send + 'static>(mut t: poll_promise::Promise<T>) -> T {
    loop {
        t = match t.try_take() {
            Ok(t) => break t,
            Err(t) => {
                tokio::task::yield_now().await;
                t
            }
        };
    }
}

pub fn button_click_event(b: UscButton) -> Vec<UscInputEvent> {
    vec![
        UscInputEvent::Button(
            b,
            ElementState::Pressed,
            SystemTime::now() - Duration::from_millis(10),
        ),
        UscInputEvent::Button(
            b,
            ElementState::Released,
            SystemTime::now() - Duration::from_millis(10),
        ),
    ]
}

pub trait ServiceHelper {
    fn add_worker<T: WorkerService + 'static>(&mut self) -> &mut Self;
}

impl ServiceHelper for ServiceCollection {
    fn add_worker<T: WorkerService + 'static>(&mut self) -> &mut Self {
        self.add(transient_factory::<RwLock<dyn WorkerService>, _>(|sp| {
            sp.get_required_mut::<T>()
        }))
    }
}
#[cfg(not(target_os = "android"))]
pub struct AsyncPicker(rfd::AsyncFileDialog, bool);

#[cfg(not(target_os = "android"))]
#[allow(unused)]
impl AsyncPicker {
    pub fn new() -> Self {
        Self(AsyncFileDialog::new(), false)
    }

    pub fn set_can_create_directories(mut self, can: bool) -> Self {
        Self(self.0.set_can_create_directories(can), self.1)
    }

    pub fn set_directory<P: AsRef<std::path::Path>>(mut self, path: P) -> Self {
        Self(self.0.set_directory(path), self.1)
    }

    pub fn set_title(mut self, title: impl Into<String>) -> Self {
        Self(self.0.set_title(title), self.1)
    }

    pub fn set_file_name(mut self, file_name: impl Into<String>) -> Self {
        Self(self.0.set_file_name(file_name), self.1)
    }

    pub fn folder(mut self) -> Self {
        self.1 = false;
        self
    }
    pub fn file(mut self) -> Self {
        self.1 = true;
        self
    }

    pub fn show<S: egui::widgets::text_edit::TextBuffer>(
        self,
        id: egui::Id,
        s: &mut S,
        ui: &mut egui::Ui,
    ) {
        type Dialog = Arc<Mutex<poll_promise::Promise<Option<rfd::FileHandle>>>>;
        let task = ui
            .data_mut(|x| x.remove_temp::<Option<Dialog>>(id))
            .flatten();
        ui.text_edit_singleline(s);
        if ui
            .add_enabled(task.is_none(), egui::Button::new("..."))
            .clicked()
        {
            ui.data_mut(|x| {
                x.insert_temp::<Option<Dialog>>(
                    id,
                    Some(Arc::new(Mutex::new(poll_promise::Promise::spawn_async(
                        async move {
                            if self.1 {
                                self.0.pick_file().await
                            } else {
                                self.0.pick_folder().await
                            }
                        },
                    )))),
                );
            })
        }

        let completed = if let Some(task) = task.clone() {
            let mut task = task.lock().unwrap();
            match task.poll_mut() {
                std::task::Poll::Ready(x) => {
                    if let Some(f) = x.take() {
                        log::info!("Picked file/folder: {:?}", f.path());
                        s.replace_with(f.path().to_str().unwrap_or(""))
                    }
                    true
                }
                std::task::Poll::Pending => false,
            }
        } else {
            false
        };

        if !completed && task.is_some() {
            ui.data_mut(|x| {
                x.insert_temp(id, task);
            })
        }
    }

    pub fn add_filter(mut self, name: impl Into<String>, extensions: &[impl ToString]) -> Self {
        Self(self.0.add_filter(name, extensions), self.1)
    }
}

pub fn transform_shader(mut s: String) -> String {
    thread_local! {
        static LAYOUT_REGEX: std::cell::OnceCell<regex::Regex> = const { std::cell::OnceCell::new() };
    }

    LAYOUT_REGEX
        .try_with(|cell| {
            let regex = cell.get_or_init(|| {
                regex::Regex::new(r"layout\(location=[0-9]+\)\W*").expect("Failed to compile regex")
            });

            s = regex.replace_all(&s, "").into_owned();
        })
        .expect("Could not run shader regex");

    s.lines()
        .filter(|x| !x.starts_with("#version"))
        .filter(|x| !x.starts_with("#extension"))
        .join("\n")
}

pub fn take_screenshot(
    vgfx: &Vgfx,
    area: Option<((usize, usize), (usize, usize))>,
) -> anyhow::Result<PathBuf> {
    let img = vgfx
        .canvas
        .try_lock()
        .map_err(|_| anyhow!("Failed to lock vgfx"))?
        .screenshot()?;

    let img = if let Some(((x, y), (w, h))) = area {
        img.sub_image(x, y, w, h).to_owned()
    } else {
        img.as_ref()
    };

    let (buf, width, height) = img.to_contiguous_buf();
    let config = GameConfig::get();
    let mut path = config.game_folder.clone();

    if config.screenshot_path.is_absolute() {
        path = config.screenshot_path.clone();
    } else {
        path.push(&config.screenshot_path);
    }

    std::fs::create_dir_all(&path)?;

    let timestamp = chrono::Local::now();

    path.push(timestamp.format("%Y-%m-%d_%H-%M-%S.png").to_string());

    image::save_buffer(
        &path,
        unsafe { std::mem::transmute(buf.as_ref()) },
        width as _,
        height as _,
        image::ColorType::Rgba8,
    )?;

    Ok(path
        .strip_prefix(&config.game_folder)
        .map(|x| x.to_path_buf())
        .unwrap_or(path))
}

pub fn wait_until(frame_end: SystemTime) {
    let mut now = SystemTime::now();
    if now > frame_end {
        return;
    }
    let ms = Duration::from_millis(1);
    while now < frame_end {
        let wait = frame_end.duration_since(now).unwrap_or(Duration::ZERO);
        if wait > ms {
            std::thread::sleep(wait - ms);
        }
        now = SystemTime::now();
    }
}

pub enum ConfigCommand {
    SetSwapInterval(wgpu::PresentMode),
    SetSize(PhysicalSize<u32>),
}

#[derive(Clone)]
pub struct RenderContext {
    pub queue: wgpu::Queue,
    pub device: wgpu::Device,
    pub surface_ctx: Arc<wgpu::Surface<'static>>,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub gui: Arc<egui_wgpu::RenderState>,
    pub commands: Arc<Mutex<Vec<ConfigCommand>>>,
}

impl RenderContext {
    pub fn process_config(&mut self) {
        let mut changed = false;
        for c in self.commands.lock().unwrap().drain(..) {
            changed = true;
            match c {
                ConfigCommand::SetSwapInterval(present_mode) => {
                    self.surface_config.present_mode = present_mode;
                }
                ConfigCommand::SetSize(s) => {
                    self.surface_config.width = s.width;
                    self.surface_config.height = s.height;
                }
            }
        }

        if changed {
            self.surface_ctx
                .configure(&self.device, &self.surface_config);
        }
    }

    pub fn set_swap_interval(&mut self, v: wgpu::PresentMode) {
        self.commands
            .lock()
            .unwrap()
            .push(ConfigCommand::SetSwapInterval(v));
    }

    pub fn set_size(&mut self, size: PhysicalSize<u32>) {
        self.commands
            .lock()
            .unwrap()
            .push(ConfigCommand::SetSize(size));
    }

    pub fn load_texture(&self, p: impl AsRef<Path>) -> anyhow::Result<wgpu::Texture> {
        let image = image::open(p)?;

        let tex = self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: None,
                size: Extent3d {
                    width: image.width(),
                    height: image.height(),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: TextureUsages::TEXTURE_BINDING,
                view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            image.to_rgba8().as_raw(),
        );

        Ok(tex)
    }
    pub fn load_textures(
        &self,
        p: &[impl AsRef<Path>],
    ) -> anyhow::Result<HashMap<String, wgpu::Texture>> {
        Ok(p.iter()
            .map(|p| match self.load_texture(p) {
                Ok(t) => Ok((
                    p.as_ref()
                        .file_stem()
                        .unwrap()
                        .to_string_lossy()
                        .to_string(),
                    t,
                )),
                Err(e) => Err(e),
            })
            .try_collect()?)
    }

    pub fn new_screen_texture(
        &self,
        viewport: Viewport,
        surface: &SurfaceTexture,
    ) -> (wgpu::Texture, wgpu::CommandBuffer) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        let target_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: viewport.extend3d(1),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: surface.texture.format(),
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::COPY_SRC,
            view_formats: &[surface.texture.format()],
        });

        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfoBase {
                texture: &surface.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfoBase {
                texture: &target_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            viewport.extend3d(1),
        );

        (target_texture, encoder.finish())
    }

    pub fn update_screen_texture(
        &self,
        texture: Texture,
        viewport: Viewport,
        surface: SurfaceTexture,
    ) {
        let mut encoder = self.encoder(None);

        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfoBase {
                texture: &surface.texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfoBase {
                texture: &texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            viewport.extend3d(1),
        );
        self.queue.submit([encoder.finish()]);
    }

    pub fn default_render_pass<'a>(
        &self,
        name: Option<&str>,
        encoder: &'a mut wgpu::CommandEncoder,
        surface: &SurfaceTexture,
    ) -> wgpu::RenderPass<'a> {
        let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: name,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &surface.texture.create_view(&Default::default()),
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        render_pass
    }

    /// Clears the surface texture to black, should be the first command buffer of a frame
    pub fn clear_surface(&self, surface: &wgpu::Texture) -> wgpu::CommandBuffer {
        self.clear_texture(surface, wgpu::Color::BLACK)
    }

    pub fn clear_texture(&self, texture: &wgpu::Texture, color: wgpu::Color) -> wgpu::CommandBuffer {
        let mut encoder = self.encoder(Some("Clear texture"));
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Clear texture"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &texture.create_view(&Default::default()),
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        encoder.finish()
    }

    pub fn encoder(&self, desc: Option<&str>) -> wgpu::CommandEncoder {
        self.device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: desc })
    }

    pub(crate) fn new_empty_screen_texture(&self, label: Option<&str>) -> wgpu::Texture {
        self.new_screen_format_texture(
            label,
            Extent3d {
                width: self.surface_config.width,
                height: self.surface_config.height,
                depth_or_array_layers: 1,
            },
        )
    }

    /// Creates a texture with the same format as the surface that can be rendered to and sampled
    pub(crate) fn new_screen_format_texture(
        &self,
        label: Option<&str>,
        size: Extent3d,
    ) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_config.format,
            usage: TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::COPY_SRC
                | TextureUsages::TEXTURE_BINDING,
            view_formats: &self.surface_config.view_formats,
        })
    }

    pub(crate) fn new_render_target(&self, label: Option<&str>) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label,
            size: Extent3d {
                width: self.surface_config.width,
                height: self.surface_config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_config.format,
            usage: self.surface_config.usage | TextureUsages::COPY_DST | TextureUsages::COPY_SRC,
            view_formats: &self.surface_config.view_formats,
        })
    }
}

pub(crate) const fn blend_add() -> wgpu::BlendState {
    let color_comp = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    };

    // Min/max blend operations require `One` factors, OpenGL ignores the factors for them anyway
    let alpha_comp = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Max,
    };

    wgpu::BlendState {
        color: color_comp,
        alpha: alpha_comp,
    }
}

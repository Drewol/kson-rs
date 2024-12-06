use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, SystemTime},
};

use anyhow::anyhow;
use di::{transient_factory, RefMut, ServiceCollection};
use femtovg::rgb::ComponentSlice;
use itertools::Itertools;
use rfd::AsyncFileDialog;
use tealr::{
    mlu::{
        mlua::{self, FromLuaMulti, IntoLuaMulti, Lua, Result},
        MaybeSend,
    },
    TealMultiValue, ToTypename,
};
use wgpu::{
    util::DeviceExt, Extent3d, Origin3d, SurfaceConfiguration, SurfaceTexture, Texture,
    TextureFormat, TextureUsages,
};
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

pub(crate) fn add_lua_static_method<'lua, M, A, R, F, T: 'static + Sized + ToTypename>(
    methods: &mut M,
    name: &'static str,
    mut function: F,
) where
    M: Sized + tealr::mlu::TealDataMethods<'lua, T>,
    A: FromLuaMulti<'lua> + TealMultiValue,
    R: IntoLuaMulti<'lua> + TealMultiValue,
    F: 'static + MaybeSend + FnMut(&'lua Lua, &mut T, A) -> Result<R>,
{
    let scope_id = puffin::ThreadProfiler::call(|f| f.register_function_scope(name, "Lua", 0));

    methods.add_function_mut(name, move |lua, p: A| {
        let _profile_scope = if puffin::are_scopes_on() && !name.ends_with("Profile") {
            Some(puffin::ProfilerScope::new(
                scope_id,
                format!(
                    "{}:{}",
                    lua.inspect_stack(1)
                        .and_then(|s| s.source().source.map(|s| s.to_string()))
                        .unwrap_or_default(),
                    lua.inspect_stack(1).map(|s| s.curr_line()).unwrap_or(-1)
                ),
            ))
        } else {
            None
        };

        let mut maybe_data = { lua.app_data_ref::<RefMut<T>>().map(|x| x.clone()) };
        if let Some(data_rc) = maybe_data.take() {
            let data = data_rc.clone();
            drop(data_rc);
            drop(maybe_data);
            let data_lock = data.try_write();
            match data_lock {
                Ok(mut data) => function(lua, &mut data, p),
                Err(e) => Err(mlua::Error::external(format!("{e}"))),
            }
        } else {
            Err(mlua::Error::external("App data not set"))
        }
    })
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

pub struct AsyncPicker(rfd::AsyncFileDialog, bool);

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
                )
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
            ui.data_mut(|x| x.insert_temp(id, task))
        }
    }

    pub fn add_filter(mut self, name: impl Into<String>, extensions: &[impl ToString]) -> Self {
        Self(self.0.add_filter(name, extensions), self.1)
    }
}

pub fn transform_shader(s: String) -> String {
    s.lines()
        .filter(|x| !x.starts_with("#version"))
        .filter(|x| !x.starts_with("#extension"))
        .map(|x| {
            if x.starts_with("layout") {
                let i = x[5..]
                    .find(" in ")
                    .or_else(|| x[5..].find(" out "))
                    .unwrap_or(0);
                x[(i + 5)..].trim()
            } else {
                x
            }
        })
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
        buf.as_slice(),
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

#[derive(Debug, Clone)]
pub struct RenderContext {
    pub queue: Arc<wgpu::Queue>,
    pub device: Arc<wgpu::Device>,
    pub surface_ctx: Arc<wgpu::Surface<'static>>,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub pending_screen_grabs: Arc<Mutex<Vec<Arc<Texture>>>>,
}

impl RenderContext {
    pub fn process_screen_grabs(&self, surface: &SurfaceTexture, viewport: Viewport) {
        self.device.poll(wgpu::MaintainBase::Wait);
        let mut grabs = self.pending_screen_grabs.lock().unwrap();
        let mut encoder = self.encoder(None);
        while let Some(target_texture) = grabs.pop() {
            encoder.copy_texture_to_texture(
                wgpu::ImageCopyTextureBase {
                    texture: &surface.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::ImageCopyTextureBase {
                    texture: &target_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                viewport.extend3d(1),
            );
        }
        self.queue.submit([encoder.finish()]);
    }
    pub fn set_swap_interval(&mut self, v: wgpu::PresentMode) {
        self.surface_config.present_mode = v;

        self.surface_ctx
            .configure(&self.device, &self.surface_config);
    }

    pub fn set_size(&mut self, size: PhysicalSize<u32>) {
        self.surface_config.width = size.width;
        self.surface_config.height = size.height;

        self.surface_ctx
            .configure(&self.device, &self.surface_config);
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
            image
                .as_rgba8()
                .ok_or(anyhow!("Could not convert image"))?
                .as_raw(),
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
    ) -> Arc<wgpu::Texture> {
        self.device.poll(wgpu::MaintainBase::Wait);
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

        let ret = Arc::new(target_texture);
        self.pending_screen_grabs.lock().unwrap().push(ret.clone());
        ret
    }

    pub fn update_screen_texture(
        &self,
        texture: Texture,
        viewport: Viewport,
        surface: SurfaceTexture,
    ) {
        let mut encoder = self.encoder(None);

        encoder.copy_texture_to_texture(
            wgpu::ImageCopyTextureBase {
                texture: &surface.texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyTextureBase {
                texture: &texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            viewport.extend3d(1),
        );
        self.queue.submit([encoder.finish()]);
    }

    pub fn encoder(&self, desc: Option<&str>) -> wgpu::CommandEncoder {
        self.device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: desc })
    }
}

pub(crate) const fn blend_add() -> wgpu::BlendState {
    let add_comp = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    };
    wgpu::BlendState {
        color: add_comp,
        alpha: add_comp,
    }
}

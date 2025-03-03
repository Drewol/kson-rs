use std::{
    borrow::Borrow,
    collections::HashMap,
    hash::{Hash, Hasher},
    io::BufReader,
    ops::{Deref, DerefMut},
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};

const COMPAT_TEXT_SCALE: f32 = 21.5 / 30.0; // Needed because old usc has two different text rendering methods for text, and fasttext/labels

use anyhow::anyhow;
use di::{Activator, InjectBuilder, Injectable, RefMut};
use femtovg::{renderer::OpenGl, Canvas, Color, FontId, ImageFlags, ImageId, Paint, Path};

use log::warn;
use poll_promise::Promise;
use puffin::profile_scope;

type LuaError = mlua::Error;
use three_d::Vector3;

use crate::{
    animation::VgAnimation, config::GameConfig, default_game_dir, log_result, lua_service::LuaKey,
    settings_screen::skin_select::SkinMeta, shaded_mesh::ShadedMesh, util::lua_address,
};
use mlua::{self, Lua, UserData};

const FALLBACK_ID: u32 = u32::MAX;

#[derive(Debug)]
enum VgImage {
    Static(ImageId),
    Animation(VgAnimation),
}

impl VgImage {
    fn current_id(&self) -> Option<ImageId> {
        match self {
            VgImage::Static(id) => Some(*id),
            VgImage::Animation(id) => id.current_img_id(),
        }
    }
}

fn unimplemented() -> mlua::Result<()> {
    Err(mlua::Error::RuntimeError(
        "Function not implemented".to_string(),
    ))
}

#[inline]
fn hash_path(path: &Path) -> u64 {
    path.verbs()
        .fold(egui::ahash::AHasher::default(), |mut h, v| {
            match v {
                femtovg::Verb::MoveTo(x, y) => (0u32, x.to_bits(), y.to_bits()).hash(&mut h),
                femtovg::Verb::LineTo(x, y) => (1u32, x.to_bits(), y.to_bits()).hash(&mut h),
                femtovg::Verb::BezierTo(a, b, c, d, e, f) => (
                    2u32,
                    a.to_bits(),
                    b.to_bits(),
                    c.to_bits(),
                    d.to_bits(),
                    e.to_bits(),
                    f.to_bits(),
                )
                    .hash(&mut h),
                femtovg::Verb::Solid => 3u32.hash(&mut h),
                femtovg::Verb::Hole => 4u32.hash(&mut h),
                femtovg::Verb::Close => 5u32.hash(&mut h),
            }
            h
        })
        .finish()
}

struct ScopedAssets {
    images: HashMap<u32, VgImage>,
    paints: HashMap<u32, Paint>,
    labels: HashMap<u32, Label>,
    paint_imgs: HashMap<u32, ImageId>,
    job_imgs: HashMap<String, u32>,
    canvas: Arc<Mutex<Canvas<OpenGl>>>,
}

impl ScopedAssets {
    fn new(canvas: Arc<Mutex<Canvas<OpenGl>>>) -> Self {
        Self {
            images: Default::default(),
            paints: Default::default(),
            labels: Default::default(),
            paint_imgs: Default::default(),
            job_imgs: Default::default(),
            canvas,
        }
    }
}

impl Drop for ScopedAssets {
    fn drop(&mut self) {
        if let Ok(mut canvas) = self.canvas.lock() {
            self.images.iter().for_each(|(_, img)| match img {
                VgImage::Static(id) => canvas.delete_image(*id),
                VgImage::Animation(anim) => anim.delete_imgs(&mut canvas),
            });

            self.paint_imgs
                .iter()
                .for_each(|(_, id)| canvas.delete_image(*id));
        }
    }
}

struct VgfxPoint {
    path: Option<Path>,
    fill_paint: Option<Paint>,
    stroke_paint: Paint,
    image_tint: Option<Color>,
}

pub struct Vgfx {
    pub canvas: Arc<Mutex<Canvas<OpenGl>>>,
    skin: String,
    _skin_meta: SkinMeta,
    restore_stack: Vec<VgfxPoint>,
    path: Option<Path>,
    fill_paint: Option<Paint>,
    path_cache: HashMap<u64, Path>,
    image_tint: Option<Color>,
    label_color: Color, // Has some strange behaviour but needed for compat
    label_font: FontId,
    stroke_paint: Paint,
    gradient_colors: [Color; 2],
    game_folder: std::path::PathBuf,
    next_img_id: u32,
    next_paint_id: u32,
    next_label_id: u32,
    scoped_assets: HashMap<usize, ScopedAssets>,
    fonts: HashMap<String, FontId>,
    image_jobs: HashMap<String, Promise<image::DynamicImage>>,
    label_align: (femtovg::Align, femtovg::Baseline),
}

impl Injectable for Vgfx {
    fn inject(lifetime: di::ServiceLifetime) -> di::InjectBuilder {
        InjectBuilder::new(
            Activator::new::<Self, Self>(
                |sp| Arc::new(Self::new(sp.get_required(), default_game_dir())),
                |sp| Arc::new(Self::new(sp.get_required(), default_game_dir()).into()),
            ),
            lifetime,
        )
    }
}

#[derive(Clone, Debug)]
struct Label {
    text: String,
    size: i32,
    _monospace: bool,
    font: FontId,
}

impl Vgfx {
    pub fn new(canvas: Arc<Mutex<Canvas<OpenGl>>>, game_folder: std::path::PathBuf) -> Self {
        let default_fonts = {
            let mut canvas = canvas.lock().expect("Lock error");

            let mut font_dir = game_folder.clone();
            font_dir.push("fonts");
            let default_fonts = canvas
                .add_font_dir(&font_dir)
                .expect("Failed to load default fonts");
            font_dir.push("settings");
            _ = canvas
                .add_font_dir(&font_dir)
                .expect("Failed to load settings fonts");

            default_fonts
        };

        let config = &GameConfig::get();
        let mut meta_file = config.skin_path();
        meta_file.push("meta.json");
        let skin_meta = std::fs::File::open(&meta_file)
            .map(BufReader::new)
            .ok()
            .and_then(|r| serde_json::from_reader::<_, SkinMeta>(r).ok())
            .unwrap_or_default();

        Self {
            path_cache: Default::default(),
            restore_stack: vec![],
            canvas,
            game_folder,
            skin: config.skin.clone(),
            path: Some(Path::new()),
            fill_paint: None,
            stroke_paint: Paint::color(Color::white()),
            gradient_colors: [Color::black(), Color::black()],
            fonts: Default::default(),
            next_img_id: 1,
            next_paint_id: 1,
            next_label_id: 1,
            image_jobs: Default::default(),
            scoped_assets: Default::default(),
            image_tint: None,
            label_color: Color::white(),
            label_font: *default_fonts.first().expect("No default font loaded"),
            label_align: (femtovg::Align::Left, femtovg::Baseline::Alphabetic),
            _skin_meta: skin_meta,
        }
    }

    pub fn drop_assets(&mut self, lua_index: usize) {
        let removed_assets = self.scoped_assets.remove(&lua_index);
        //TODO: Call deleteimage on canvas for removed images
        if let Some(removed_assets) = removed_assets {
            log::info!(
                "Dropped assets:\n  {} Images/Animation\n  {} Labels",
                removed_assets.images.len(),
                removed_assets.labels.len()
            );

            //Just clear cache here, first frame of restored scene will take longer but most important stuff should be cached quickly
            //TODO: Do something smarter?
            self.path_cache.clear();
        }
    }

    pub fn init_asset_scope(&mut self, lua_index: usize) {
        self.scoped_assets
            .insert(lua_index, ScopedAssets::new(self.canvas.clone()));
    }

    fn with_canvas<R>(
        &mut self,
        mut f: impl FnMut(&mut Canvas<OpenGl>) -> R,
    ) -> Result<R, mlua::Error> {
        let canvas = &mut self
            .canvas
            .try_lock()
            .map_err(|_| mlua::Error::external("Canvas in use".to_string()))?;

        Ok(f(canvas))
    }

    pub fn load_image(
        &mut self,
        path: impl AsRef<std::path::Path>,
        lua_index: usize,
    ) -> anyhow::Result<u32> {
        let img = self.with_canvas(|x| x.load_image_file(&path, ImageFlags::empty()))??;
        self.scoped_assets
            .get_mut(&lua_index)
            .ok_or(anyhow!("Assets not initialized"))?
            .images
            .insert(self.next_img_id, VgImage::Static(img));
        let result = self.next_img_id;
        self.next_img_id += 1;

        Ok(result)
    }

    pub fn delete_image(&mut self, image: u32, lua_index: usize) {
        if let Some(VgImage::Static(id)) = self.scoped_assets[&lua_index].images.get(&image) {
            let id = *id;
            log_result!(self.with_canvas(|x| x.delete_image(id)));
        }
    }

    pub fn skin_folder(&self) -> PathBuf {
        let mut res = self.game_folder.clone();
        res.push("skins");
        res.push(&self.skin);
        res
    }
}

use mlua_bridge::mlua_bridge;

pub struct VgfxLua;

#[mlua_bridge(rename_funcs = "PascalCase", no_auto_fields)]
impl VgfxLua {
    fn begin_path(_vgfx: &RefMut<Vgfx>) -> Result<(), LuaError> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        _vgfx.path = Some(Path::new());
        _vgfx.label_color = Color::white();

        Ok(())
    }

    fn rect(_vgfx: &RefMut<Vgfx>, x: f32, y: f32, w: f32, h: f32) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        match _vgfx.path.as_mut() {
            Some(p) => {
                p.rect(x, y, w, h);
                Ok(())
            }
            None => Err(mlua::Error::external("No path begun".to_string())),
        }
    }

    fn fast_rect(_vgfx: &RefMut<Vgfx>, x: f32, y: f32, w: f32, h: f32) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        if let Some(paint) = _vgfx.fill_paint.as_ref() {
            let mut p = Path::new();
            p.rect(x, y, w, h);
            _vgfx
                .canvas
                .lock()
                .expect("Lock error")
                .fill_path(&p, paint);
        }
        Ok(())
    }

    fn fill(_vgfx: &RefMut<Vgfx>) -> Result<(), LuaError> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        match (_vgfx.path.as_ref(), _vgfx.fill_paint.as_ref()) {
            (Some(path), Some(paint)) => {
                let path = {
                    profile_scope!("Path Cache");
                    let path_hash = hash_path(path);
                    _vgfx.path_cache.entry(path_hash).or_insert(path.clone())
                };
                let canvas = &mut _vgfx
                    .canvas
                    .try_lock()
                    .map_err(|_| mlua::Error::external("Canvas in use".to_string()))?;
                canvas.fill_path(path, paint);
                Ok(())
            }
            (None, None) => Err(mlua::Error::external(
                "No path begun and no paint set".to_string(),
            )),
            (None, Some(_)) => Err(mlua::Error::external("No path begun".to_string())),
            (Some(_), None) => Err(mlua::Error::external("No paint set".to_string())),
        }
    }

    fn fill_color(
        _vgfx: &RefMut<Vgfx>,
        r: u8,
        g: u8,
        b: u8,
        a: Option<u8>,
    ) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        let color = Color::rgba(r, g, b, a.unwrap_or(255));
        _vgfx.label_color = color;
        if let Some(paint) = _vgfx.fill_paint.as_mut() {
            paint.set_color(color);
        } else {
            _vgfx.fill_paint = Some(Paint::color(color));
        }
        Ok(())
    }

    fn create_image(
        lua: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        filename: String,
        imageflags: u32,
    ) -> Result<u32, mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        let Ok(img) = _vgfx
            .with_canvas(|canvas| {
                canvas.load_image_file(
                    &filename,
                    ImageFlags::from_bits(imageflags).unwrap_or(ImageFlags::empty()),
                )
            })?
            .map_err(mlua::Error::external)
        else {
            return Ok(0);
        };

        let this_id = _vgfx.next_img_id;
        _vgfx.next_img_id += 1;
        _vgfx
            .scoped_assets
            .get_mut(&lua.key())
            .ok_or(mlua::Error::external("Assets not initialized"))?
            .images
            .insert(this_id, VgImage::Static(img));
        Ok(this_id)
    }

    fn create_skin_image(
        lua: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        filename: String,
        imageflags: u32,
    ) -> Result<std::option::Option<u32>, mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        let mut path = _vgfx.game_folder.clone();
        path.push("skins");
        path.push(&_vgfx.skin);
        path.push("textures");
        path.push(&filename);
        let img = match _vgfx.with_canvas(|canvas| {
            canvas
                .load_image_file(
                    &path,
                    ImageFlags::from_bits(imageflags).unwrap_or(ImageFlags::empty()),
                )
                .or_else(|_| {
                    profile_scope!("reformat image");
                    let img = image::open(&path)?;
                    canvas.create_image(
                        femtovg::ImageSource::try_from(&image::DynamicImage::ImageRgba8(
                            img.to_rgba8(),
                        ))
                        .expect("Bad image format"),
                        ImageFlags::from_bits(imageflags).unwrap_or(ImageFlags::empty()),
                    )
                })
        })? {
            Ok(img) => img,
            Err(err) => {
                log::error!("Failed to load image \"{}\": {:?}", &filename, err);
                return Ok(None);
            }
        };

        let this_id = _vgfx.next_img_id;
        _vgfx.next_img_id += 1;
        _vgfx
            .scoped_assets
            .get_mut(&lua.key())
            .ok_or(mlua::Error::external("Assets not initialized"))?
            .images
            .insert(this_id, VgImage::Static(img));
        Ok(Some(this_id))
    }

    fn image_rect(
        lua: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        image: u32,
        alpha: f32,
        angle: f32,
    ) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        if image == FALLBACK_ID {
            return Ok(());
        }

        if let Some(img_id) = _vgfx.scoped_assets[&lua.key()]
            .images
            .get(&image)
            .and_then(|x| x.current_id())
        {
            let tint = _vgfx.image_tint;
            _vgfx.with_canvas(|canvas| {
                canvas.save_with(|canvas| {
                    let (img_w, img_h) = canvas
                        .image_size(img_id)
                        .map_err(mlua::Error::external)
                        .unwrap_or((1, 1));
                    let scale_x = w / img_w as f32;
                    let scale_y = h / img_h as f32;
                    canvas.translate(x, y);
                    canvas.rotate(angle);
                    canvas.scale(scale_x, scale_y);
                    let paint = if let Some(mut tint) = tint {
                        tint.set_alphaf(alpha);
                        Paint::image_tint(img_id, 0.0, 0.0, img_w as f32, img_h as f32, 0.0, tint)
                    } else {
                        Paint::image_tint(
                            img_id,
                            0.0,
                            0.0,
                            img_w as f32,
                            img_h as f32,
                            0.0,
                            Color {
                                r: 1.0,
                                g: 1.0,
                                b: 1.0,
                                a: alpha,
                            },
                        )
                    };
                    let mut rect = Path::new();
                    rect.rect(0.0, 0.0, img_w as f32, img_h as f32);
                    canvas.fill_path(&rect, &paint);
                });
            })
        } else {
            Ok(())
        }
    }

    fn text(_vgfx: &RefMut<Vgfx>, s: Option<String>, x: f32, y: f32) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        let Some(s) = s else {
            return Ok(());
        };
        match _vgfx.fill_paint.as_ref() {
            Some(fill_paint) => {
                let canvas = &mut _vgfx
                    .canvas
                    .try_lock()
                    .map_err(|_| mlua::Error::external("Canvas in use".to_string()))?;

                canvas
                    .fill_text(x, y, s, fill_paint)
                    .map_err(mlua::Error::external)?;
                Ok(())
            }
            None => todo!(),
        }
    }

    fn text_align(_vgfx: &RefMut<Vgfx>, align: u32) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        let align = TextAlign::from_bits(align)
            .unwrap_or(TextAlign::ALIGN_BASELINE | TextAlign::ALIGN_LEFT);
        let vertical = match align & TextAlign::VERTICAL {
            TextAlign::ALIGN_BOTTOM => femtovg::Baseline::Bottom,
            TextAlign::ALIGN_MIDDLE => femtovg::Baseline::Middle,
            TextAlign::ALIGN_TOP => femtovg::Baseline::Top,
            _ => femtovg::Baseline::Alphabetic,
        };

        let horizontal = match align & TextAlign::HORIZONTAL {
            TextAlign::ALIGN_CENTER => femtovg::Align::Center,
            TextAlign::ALIGN_RIGHT => femtovg::Align::Right,
            _ => femtovg::Align::Left,
        };

        _vgfx.label_align = (horizontal, vertical);
        _vgfx.stroke_paint.set_text_align(horizontal);
        _vgfx.stroke_paint.set_text_baseline(vertical);
        if let Some(text_paint) = _vgfx.fill_paint.as_mut() {
            text_paint.set_text_align(horizontal);
            text_paint.set_text_baseline(vertical);
        }
        Ok(())
    }

    fn font_face(_vgfx: &RefMut<Vgfx>, s: String) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        if let Some(font_id) = _vgfx.fonts.get(&s) {
            _vgfx.label_font = *font_id;
            if let Some(text_paint) = _vgfx.fill_paint.as_mut() {
                text_paint.set_font(&[*font_id]);
            }
        } else {
            warn!("No loaded font named: {}", &s)
        }
        Ok(())
    }

    fn font_size(_vgfx: &RefMut<Vgfx>, size: f32) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        if let Some(text_paint) = _vgfx.fill_paint.as_mut() {
            text_paint.set_font_size(size * COMPAT_TEXT_SCALE);
        }
        Ok(())
    }

    fn translate(_vgfx: &RefMut<Vgfx>, x: f32, y: f32) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        _vgfx.with_canvas(|canvas| canvas.translate(x, y))?;
        Ok(())
    }

    fn scale(_vgfx: &RefMut<Vgfx>, x: f32, y: f32) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        _vgfx.with_canvas(|canvas| canvas.scale(x, y))?;
        Ok(())
    }

    fn rotate(_vgfx: &RefMut<Vgfx>, angle: f32) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        _vgfx.with_canvas(|canvas| canvas.rotate(angle))?;
        Ok(())
    }

    fn reset_transform(_vgfx: &RefMut<Vgfx>) -> Result<(), LuaError> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        _vgfx.with_canvas(|canvas| canvas.reset_transform())?;
        Ok(())
    }

    fn load_font(
        _vgfx: &RefMut<Vgfx>,
        name: String,
        filename: Option<String>,
    ) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        let name = name;
        if let (Some(font_id), Some(paint)) = (_vgfx.fonts.get(&name), _vgfx.fill_paint.as_mut()) {
            paint.set_font(&[*font_id]);
            _vgfx.label_font = *font_id;
        } else {
            let path = filename.unwrap_or_else(|| name.clone());
            let font_id = _vgfx
                .with_canvas(|canvas| canvas.add_font(&path))?
                .map_err(mlua::Error::external)?;
            _vgfx.label_font = font_id;
            if let Some(paint) = _vgfx.fill_paint.as_mut() {
                paint.set_font(&[font_id]);
            }
            _vgfx.fonts.insert(name, font_id);
        }

        Ok(())
    }

    fn load_skin_font(
        _vgfx: &RefMut<Vgfx>,
        name: String,
        filename: Option<String>,
    ) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        let name = name;
        if let (Some(font_id), Some(paint)) = (_vgfx.fonts.get(&name), _vgfx.fill_paint.as_mut()) {
            paint.set_font(&[*font_id]);
            _vgfx.label_font = *font_id;
        } else {
            let path = filename.unwrap_or_else(|| name.clone());
            let mut font_path = _vgfx.game_folder.clone();
            font_path.push("skins");
            font_path.push(&_vgfx.skin);
            font_path.push("fonts");
            font_path.push(path);

            let font_id = _vgfx
                .with_canvas(|canvas| canvas.add_font(&font_path))?
                .map_err(mlua::Error::external)?;
            _vgfx.label_font = font_id;

            if let Some(paint) = _vgfx.fill_paint.as_mut() {
                paint.set_font(&[font_id]);
            }
            _vgfx.fonts.insert(name, font_id);
        }

        Ok(())
    }

    fn fast_text(
        _vgfx: &RefMut<Vgfx>,
        input_text: String,
        x: f32,
        y: f32,
    ) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        match _vgfx.fill_paint.as_ref() {
            Some(fill_paint) => {
                let canvas = &mut _vgfx
                    .canvas
                    .try_lock()
                    .map_err(|_| mlua::Error::external("Canvas in use".to_string()))?;
                canvas
                    .fill_text(
                        x,
                        y,
                        input_text,
                        &fill_paint
                            .clone()
                            .with_font_size(fill_paint.font_size() / COMPAT_TEXT_SCALE),
                    )
                    .map_err(mlua::Error::external)?;
                Ok(())
            }
            None => todo!(),
        }
    }

    fn create_label(
        lua: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        text: Option<String>,
        size: i32,
        monospace: bool,
    ) -> Result<u32, mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        _vgfx
            .scoped_assets
            .get_mut(&lua.key())
            .ok_or(mlua::Error::external("Assets not initialized"))?
            .labels
            .insert(
                _vgfx.next_label_id,
                Label {
                    text: text.unwrap_or_default(),
                    size,
                    _monospace: monospace,
                    font: _vgfx.label_font,
                },
            );

        let id = _vgfx.next_label_id;
        _vgfx.next_label_id += 1;

        Ok(id)
    }

    fn draw_label(
        lua: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        label_id: u32,
        x: f32,
        y: f32,
        max_width: Option<f32>,
    ) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        if let Some(label) = _vgfx.scoped_assets[&lua.key()].labels.get(&label_id) {
            let canvas = &mut _vgfx
                .canvas
                .try_lock()
                .map_err(|_| mlua::Error::external("Canvas in use".to_string()))?;
            let mut paint = _vgfx
                .fill_paint
                .clone()
                .unwrap_or_else(|| _vgfx.stroke_paint.clone())
                .with_font(&[label.font])
                .with_font_size(label.size as f32)
                .with_color(_vgfx.label_color)
                .with_text_align(_vgfx.label_align.0)
                .with_text_baseline(_vgfx.label_align.1);

            let text_measure = canvas
                .measure_text(x, y, &label.text, &paint)
                .map_err(mlua::Error::external)?;

            let x_scale = match max_width {
                Some(max_width) if max_width <= 0.0 => 1.0,
                Some(max_width) => (max_width / text_measure.width()).min(1.0),
                None => 1.0,
            };

            paint.set_font_size(label.size as f32 * x_scale);

            canvas
                .fill_text(x, y, &label.text, &paint)
                .map_err(mlua::Error::external)?;
        }
        Ok(())
    }

    fn move_to(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        x: f32,
        y: f32,
    ) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        if let Some(path) = _vgfx.path.as_mut() {
            path.move_to(x, y);
            Ok(())
        } else {
            Err(mlua::Error::external("No path started".to_string()))
        }
    }

    fn line_to(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        x: f32,
        y: f32,
    ) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        if let Some(path) = _vgfx.path.as_mut() {
            path.line_to(x, y);
            Ok(())
        } else {
            Err(mlua::Error::external("No path started".to_string()))
        }
    }

    fn bezier_to(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        c_1x: f32,
        c_1y: f32,
        c_2x: f32,
        c_2y: f32,
        x: f32,
        y: f32,
    ) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        if let Some(path) = _vgfx.path.as_mut() {
            path.bezier_to(c_1x, c_1y, c_2x, c_2y, x, y);
            Ok(())
        } else {
            Err(mlua::Error::external("No path started".to_string()))
        }
    }

    fn quad_to(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        cx: f32,
        cy: f32,
        x: f32,
        y: f32,
    ) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        if let Some(path) = _vgfx.path.as_mut() {
            path.quad_to(cx, cy, x, y);
            Ok(())
        } else {
            Err(mlua::Error::external("No path started".to_string()))
        }
    }

    fn arc_to(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        x_1: f32,
        y_1: f32,
        x_2: f32,
        y_2: f32,
        radius: f32,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        if let Some(path) = _vgfx.path.as_mut() {
            path.arc_to(x_1, y_1, x_2, y_2, radius);
            Ok(())
        } else {
            Err(mlua::Error::external("No path started".to_string()))
        }
    }

    fn close_path(_lua_index: &LuaKey, _vgfx: &RefMut<Vgfx>) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        if let Some(path) = _vgfx.path.as_mut() {
            path.close();
            Ok(())
        } else {
            Err(mlua::Error::external("No path started".to_string()))
        }
    }

    fn miter_limit(_lua_index: &LuaKey, _vgfx: &RefMut<Vgfx>, limit: f32) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        _vgfx.stroke_paint.set_miter_limit(limit);
        Ok(())
    }

    fn stroke_width(_lua_index: &LuaKey, _vgfx: &RefMut<Vgfx>, size: f32) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        _vgfx.stroke_paint.set_line_width(size);
        Ok(())
    }

    fn line_cap(_lua_index: &LuaKey, _vgfx: &RefMut<Vgfx>, cap: u8) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        _vgfx
            .stroke_paint
            .set_line_cap(unsafe { std::mem::transmute::<u8, femtovg::LineCap>(cap) });
        Ok(())
    }

    fn line_join(_lua_index: &LuaKey, _vgfx: &RefMut<Vgfx>, join: u8) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        _vgfx
            .stroke_paint
            .set_line_join(unsafe { std::mem::transmute::<u8, femtovg::LineJoin>(join) });
        Ok(())
    }

    fn stroke(_lua_index: &LuaKey, _vgfx: &RefMut<Vgfx>) -> Result<(), LuaError> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        if let Some(path) = _vgfx.path.as_mut() {
            let path = {
                profile_scope!("Path Cache");
                let path_hash = hash_path(path);
                _vgfx.path_cache.entry(path_hash).or_insert(path.clone())
            };

            let canvas = &mut _vgfx
                .canvas
                .try_lock()
                .map_err(|_| mlua::Error::external("Canvas in use".to_string()))?;
            canvas.stroke_path(path, &_vgfx.stroke_paint);
        }
        Ok(())
    }

    fn stroke_color(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        r: u8,
        g: u8,
        b: u8,
        a: Option<u8>,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        _vgfx
            .stroke_paint
            .set_color(Color::rgba(r, g, b, a.unwrap_or(255)));
        Ok(())
    }

    fn update_label(
        lua: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        label_id: u32,
        text: String,
        size: i32,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        if let Some(label) = _vgfx
            .scoped_assets
            .get_mut(&lua.key())
            .ok_or(mlua::Error::external("Assets not initialized"))?
            .labels
            .get_mut(&label_id)
        {
            label.text = text;
            label.size = size;
            label.font = _vgfx.label_font;
            Ok(())
        } else {
            Err(mlua::Error::external(format!(
                "No label with id {}",
                label_id
            )))
        }
    }

    fn draw_gauge(_lua_index: &LuaKey, _vgfx: &RefMut<Vgfx>) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        Err(mlua::Error::external("Function removed".to_string()))
    }

    fn set_gauge_color(_lua_index: &LuaKey, _vgfx: &RefMut<Vgfx>) -> Result<(), mlua::Error> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        Err(mlua::Error::external("Function removed".to_string()))
    }

    fn rounded_rect(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        if let Some(path) = _vgfx.path.as_mut() {
            path.rounded_rect(x, y, w, h, r);
            Ok(())
        } else {
            Err(mlua::Error::external("No path started".to_string()))
        }
    }

    fn rounded_rect_varying(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        rad_top_left: f32,
        rad_top_right: f32,
        rad_bottom_right: f32,
        rad_bottom_left: f32,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        if let Some(path) = _vgfx.path.as_mut() {
            path.rounded_rect_varying(
                x,
                y,
                w,
                h,
                rad_top_left,
                rad_top_right,
                rad_bottom_right,
                rad_bottom_left,
            );
            Ok(())
        } else {
            Err(mlua::Error::external("No path started".to_string()))
        }
    }

    fn ellipse(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        if let Some(path) = _vgfx.path.as_mut() {
            path.ellipse(cx, cy, rx, ry);
            Ok(())
        } else {
            Err(mlua::Error::external("No path started".to_string()))
        }
    }

    fn circle(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        cx: f32,
        cy: f32,
        r: f32,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        if let Some(path) = _vgfx.path.as_mut() {
            path.circle(cx, cy, r);
            Ok(())
        } else {
            Err(mlua::Error::external("No path started".to_string()))
        }
    }

    fn skew_x(_lua_index: &LuaKey, _vgfx: &RefMut<Vgfx>, angle: f32) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        _vgfx.with_canvas(|canvas| canvas.skew_x(angle))?;
        Ok(())
    }

    fn skew_y(_lua_index: &LuaKey, _vgfx: &RefMut<Vgfx>, angle: f32) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        _vgfx.with_canvas(|canvas| canvas.skew_y(angle))?;
        Ok(())
    }

    fn linear_gradient(
        lua: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        sx: f32,
        sy: f32,
        ex: f32,
        ey: f32,
    ) -> mlua::Result<u32> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        _vgfx
            .scoped_assets
            .get_mut(&lua.key())
            .ok_or(mlua::Error::external("Assets not initialized"))?
            .paints
            .insert(
                _vgfx.next_paint_id,
                Paint::linear_gradient(
                    sx,
                    sy,
                    ex,
                    ey,
                    _vgfx.gradient_colors[0],
                    _vgfx.gradient_colors[1],
                ),
            );
        let id = _vgfx.next_paint_id;
        _vgfx.next_paint_id += 1;

        Ok(id)
    }

    fn box_gradient(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        f: f32,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        _vgfx.fill_paint = Some(Paint::box_gradient(
            x,
            y,
            w,
            h,
            r,
            f,
            _vgfx.gradient_colors[0],
            _vgfx.gradient_colors[1],
        ));
        Ok(())
    }

    fn radial_gradient(
        lua: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        cx: f32,
        cy: f32,
        inr: f32,
        outr: f32,
    ) -> mlua::Result<u32> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        _vgfx
            .scoped_assets
            .get_mut(&lua.key())
            .ok_or(mlua::Error::external("Assets not initialized"))?
            .paints
            .insert(
                _vgfx.next_paint_id,
                Paint::radial_gradient(
                    cx,
                    cy,
                    inr,
                    outr,
                    _vgfx.gradient_colors[0],
                    _vgfx.gradient_colors[0],
                ),
            );
        let id = _vgfx.next_paint_id;
        _vgfx.next_paint_id += 1;

        Ok(id)
    }

    fn image_pattern(
        lua: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        ox: f32,
        oy: f32,
        ex: f32,
        ey: f32,
        angle: f32,
        image: u32,
        alpha: f32,
    ) -> mlua::Result<u32> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        if image == FALLBACK_ID {
            return Ok(FALLBACK_ID);
        }

        if let Some(id) = _vgfx.scoped_assets[&lua.key()]
            .images
            .get(&image)
            .and_then(|x| x.current_id())
        {
            let paint = Paint::image(id, ox, oy, ex, ey, angle, alpha);
            _vgfx
                .scoped_assets
                .get_mut(&lua.key())
                .ok_or(mlua::Error::external("Assets not initialized"))?
                .paints
                .insert(_vgfx.next_paint_id, paint);
            let paint_id = _vgfx.next_paint_id;
            _vgfx.next_paint_id += 1;
            _vgfx
                .scoped_assets
                .get_mut(&lua.key())
                .ok_or(mlua::Error::external("Assets not initialized"))?
                .paint_imgs
                .insert(paint_id, id);
            Ok(paint_id)
        } else {
            Err(mlua::Error::external(format!("No image with id {image}")))
        }
    }

    fn update_image_pattern(
        lua: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        paint: u32,
        ox: f32,
        oy: f32,
        ex: f32,
        ey: f32,
        angle: f32,
        alpha: f32,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        let assets = _vgfx
            .scoped_assets
            .get_mut(&lua.key())
            .ok_or(mlua::Error::external("Assets not initialized"))?;
        if let (Some(pattern_paint), Some(img)) =
            (assets.paints.get_mut(&paint), assets.paint_imgs.get(&paint))
        {
            *pattern_paint = Paint::image(*img, ox, oy, ex, ey, angle, alpha);
        }
        Ok(())
    }

    fn gradient_colors(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        ri: i32,
        gi: i32,
        bi: i32,
        ai: i32,
        ro: i32,
        go: i32,
        bo: i32,
        ao: i32,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        _vgfx.gradient_colors = [
            Color::rgba(ri as u8, gi as u8, bi as u8, ai as u8),
            Color::rgba(ro as u8, go as u8, bo as u8, ao as u8),
        ];
        Ok(())
    }

    fn fill_paint(lua: &LuaKey, _vgfx: &RefMut<Vgfx>, paint: u32) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        if let Some(paint) = _vgfx.scoped_assets[&lua.key()].paints.get(&paint) {
            _vgfx.fill_paint = Some(paint.clone());
        }
        Ok(())
    }

    fn stroke_paint(lua: &LuaKey, _vgfx: &RefMut<Vgfx>, paint: u32) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        if let Some(paint) = _vgfx.scoped_assets[&lua.key()].paints.get(&paint) {
            _vgfx.stroke_paint = paint.clone();
        }

        Ok(())
    }

    fn save(_vgfx: &RefMut<Vgfx>) -> Result<(), LuaError> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        _vgfx.with_canvas(|canvas| canvas.save())?;
        _vgfx.restore_stack.push(VgfxPoint {
            image_tint: _vgfx.image_tint,
            path: _vgfx.path.clone(),
            fill_paint: _vgfx.fill_paint.clone(),
            stroke_paint: _vgfx.stroke_paint.clone(),
        });
        //TODO: stacks for custom stuff
        Ok(())
    }

    fn restore(_lua_index: &LuaKey, _vgfx: &RefMut<Vgfx>) -> Result<(), LuaError> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        _vgfx.with_canvas(|canvas| canvas.restore())?;

        if let Some(restore) = _vgfx.restore_stack.pop() {
            let VgfxPoint {
                path,
                fill_paint,
                stroke_paint,
                image_tint,
            } = restore;
            _vgfx.image_tint = image_tint;
            _vgfx.path = path;
            _vgfx.fill_paint = fill_paint;
            _vgfx.stroke_paint = stroke_paint;
        }

        //TODO: stacks for custom stuff
        Ok(())
    }

    fn reset(_lua_index: &LuaKey, _vgfx: &RefMut<Vgfx>) -> Result<(), LuaError> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        _vgfx.restore_stack.clear();
        _vgfx.image_tint = None;
        _vgfx.with_canvas(|canvas| canvas.reset())
    }

    fn path_winding(_lua_index: &LuaKey, _vgfx: &RefMut<Vgfx>, dir: i32) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        unimplemented()
    }

    fn force_render(_lua_index: &LuaKey, _vgfx: &RefMut<Vgfx>) -> Result<(), LuaError> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        //TODO: Flush game render as well
        //_vgfx.with_canvas(|canvas| canvas.flush())?;
        Ok(())
    }

    fn load_image_job(
        lua: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        path: String,
        placeholder: Option<u32>,
        w: Option<u32>,
        h: Option<u32>,
    ) -> mlua::Result<u32> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        if let Some((key, job)) = _vgfx.image_jobs.remove_entry(&path) {
            match job.try_take() {
                Ok(img) if img.width() > 0 => {
                    let img_id = _vgfx.with_canvas(|c| {
                        c.create_image(
                            femtovg::ImageSource::try_from(&img).map_err(mlua::Error::external)?,
                            ImageFlags::empty(),
                        )
                        .map_err(mlua::Error::external)
                    })??;

                    _vgfx
                        .scoped_assets
                        .get_mut(&lua.key())
                        .ok_or(mlua::Error::external("Assets not initialized"))?
                        .images
                        .insert(_vgfx.next_img_id, VgImage::Static(img_id));
                    _vgfx
                        .scoped_assets
                        .get_mut(&lua.key())
                        .ok_or(mlua::Error::external("Assets not initialized"))?
                        .job_imgs
                        .insert(key, _vgfx.next_img_id);
                    _vgfx.next_img_id += 1;
                }
                Ok(_) => {}
                Err(job) => {
                    _vgfx.image_jobs.insert(key, job);
                }
            }
        }

        let key = path.clone();
        if !_vgfx.scoped_assets[&lua.key()].job_imgs.contains_key(&path) {
            _vgfx
                .image_jobs
                .entry(path.clone())
                .or_insert_with(move || {
                    Promise::spawn_thread("load image", move || {
                        image::open(key)
                            .map(|img| {
                                if let (Some(w), Some(h)) = (w, h) {
                                    img.resize(w, h, image::imageops::FilterType::CatmullRom)
                                } else {
                                    img
                                }
                            })
                            .unwrap_or_default()
                    })
                });
            _vgfx
                .scoped_assets
                .get_mut(&lua.key())
                .ok_or(mlua::Error::external("Assets not initialized"))?
                .job_imgs
                .insert(path.clone(), placeholder.unwrap_or_default());
        }

        Ok(*_vgfx.scoped_assets[&lua.key()]
            .job_imgs
            .get(&path)
            .unwrap_or(&placeholder.unwrap_or_default()))
    }

    fn load_web_image_job(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        url: String,
        placeholder: i32,
        w: i32,
        h: i32,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        unimplemented()
    }

    fn scissor(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        _vgfx.with_canvas(|canvas| canvas.scissor(x, y, w, h))?;

        Ok(())
    }

    fn intersect_scissor(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        _vgfx.with_canvas(|canvas| canvas.intersect_scissor(x, y, w, h))?;
        Ok(())
    }

    fn reset_scissor(_lua_index: &LuaKey, _vgfx: &RefMut<Vgfx>) -> Result<(), LuaError> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        _vgfx.with_canvas(|canvas| canvas.reset_scissor())?;
        Ok(())
    }

    fn text_bounds(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        x: f32,
        y: f32,
        s: String,
    ) -> mlua::Result<(f32, f32, f32, f32)> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        if let Some(paint) = _vgfx.fill_paint.as_ref() {
            let canvas = &mut _vgfx
                .canvas
                .try_lock()
                .map_err(|_| mlua::Error::external("Canvas in use".to_string()))?;

            let bounds = canvas
                .measure_text(x, y, s, paint)
                .map_err(mlua::Error::external)?;
            Ok((
                bounds.x,
                bounds.y,
                bounds.x + bounds.width(),
                bounds.y + bounds.height(),
            ))
        } else {
            Err(mlua::Error::external("No text paint set".to_string()))
        }
    }

    fn label_size(lua: &LuaKey, _vgfx: &RefMut<Vgfx>, label: u32) -> mlua::Result<(f32, f32)> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        let mut paint = _vgfx
            .fill_paint
            .clone()
            .unwrap_or_else(|| _vgfx.stroke_paint.clone());

        let canvas = _vgfx.canvas.lock().expect("Lock error");
        if let Some(label) = _vgfx.scoped_assets[&lua.key()].labels.get(&label) {
            paint.set_font(&[label.font]);
            paint.set_font_size(label.size as f32);
            paint.set_text_align(_vgfx.label_align.0);
            paint.set_text_baseline(_vgfx.label_align.1);
            let size = canvas
                .measure_text(0.0, 0.0, &label.text, &paint)
                .map_err(mlua::Error::external)?;
            Ok((size.width(), size.height()))
        } else {
            Err(mlua::Error::RuntimeError(format!(
                "No label with id: {}",
                label
            )))
        }
    }

    fn fast_text_size(_lua_index: &LuaKey, _vgfx: &RefMut<Vgfx>, text: String) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        unimplemented()
    }

    fn image_size(lua: &LuaKey, _vgfx: &RefMut<Vgfx>, image: u32) -> mlua::Result<(usize, usize)> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        if image == FALLBACK_ID {
            return Ok((1, 1));
        }

        if let Some(id) = _vgfx.scoped_assets[&lua.key()]
            .images
            .get(&image)
            .and_then(|x| x.current_id())
        {
            _vgfx
                .with_canvas(|canvas| canvas.image_size(id))?
                .map_err(mlua::Error::external)
        } else {
            Err(mlua::Error::external(format!("No image with id {}", image)))
        }
    }

    fn arc(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        cx: f32,
        cy: f32,
        r: f32,
        a_0: f32,
        a_1: f32,
        dir: i32,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        if let Some(path) = _vgfx.path.as_mut() {
            path.arc(
                cx,
                cy,
                r,
                a_0,
                a_1,
                if dir == 0 {
                    femtovg::Solidity::Solid
                } else {
                    femtovg::Solidity::Hole
                },
            );
            Ok(())
        } else {
            Err(mlua::Error::external("No path started".to_string()))
        }
    }

    fn set_image_tint(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        r: u8,
        g: u8,
        b: u8,
    ) -> mlua::Result<(u32)> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        if let Some(_paint) = _vgfx.fill_paint.as_mut() {
            _vgfx.image_tint = Some(Color::rgb(r, g, b));
        }
        Ok(0)
    }

    fn global_composite_operation(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        op: u8,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        if op <= femtovg::CompositeOperation::Xor as u8 {
            unsafe {
                _vgfx.with_canvas(|canvas| {
                    canvas.global_composite_operation(std::mem::transmute::<
                        u8,
                        femtovg::CompositeOperation,
                    >(op))
                })?
            }
        }

        Ok(())
    }

    fn global_composite_blend_func(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        sfactor: u8,
        dfactor: u8,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        let last_factor = femtovg::BlendFactor::SrcAlphaSaturate as u8;
        if dfactor <= last_factor && sfactor <= last_factor {
            unsafe {
                _vgfx.with_canvas(|canvas| {
                    canvas.global_composite_blend_func(
                        std::mem::transmute::<u8, femtovg::BlendFactor>(sfactor),
                        std::mem::transmute::<u8, femtovg::BlendFactor>(dfactor),
                    )
                })?
            }
        }

        Ok(())
    }

    fn global_composite_blend_func_separate(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        src_rgb: i32,
        dst_rgb: i32,
        src_alpha: i32,
        dst_alpha: i32,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        unimplemented()
    }

    fn load_animation(
        lua: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        path: String,
        frametime: f64,
        loopcount: Option<usize>,
        compressed: Option<bool>,
    ) -> mlua::Result<u32> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        let anim = VgAnimation::new(
            path,
            frametime,
            _vgfx.canvas.clone(),
            loopcount.unwrap_or_default(),
            compressed.unwrap_or_default(),
        )
        .map_err(mlua::Error::external)?;
        _vgfx
            .scoped_assets
            .get_mut(&lua.key())
            .ok_or(mlua::Error::external("Assets not initialized"))?
            .images
            .insert(_vgfx.next_img_id, VgImage::Animation(anim));
        let res = _vgfx.next_img_id;
        _vgfx.next_img_id += 1;

        Ok(res)
    }

    fn global_alpha(_lua_index: &LuaKey, _vgfx: &RefMut<Vgfx>, alpha: f32) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();

        _vgfx
            .canvas
            .lock()
            .expect("Lock error")
            .set_global_alpha(alpha);
        Ok(())
    }

    fn load_skin_animation(
        lua: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        path: String,
        frametime: f64,
        loopcount: Option<usize>,
        compressed: Option<bool>,
    ) -> Result<u32, LuaError> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        let mut skinned_path = _vgfx.game_folder.clone();
        skinned_path.push("skins");
        skinned_path.push(&_vgfx.skin);
        skinned_path.push("textures");
        skinned_path.push(path);

        let anim = VgAnimation::new(
            skinned_path,
            frametime,
            _vgfx.canvas.clone(),
            loopcount.unwrap_or_default(),
            compressed.unwrap_or_default(),
        )
        .map_err(mlua::Error::external)?;
        _vgfx
            .scoped_assets
            .get_mut(&lua.key())
            .ok_or(mlua::Error::external("Assets not initialized"))?
            .images
            .insert(_vgfx.next_img_id, VgImage::Animation(anim));
        let res = _vgfx.next_img_id;
        _vgfx.next_img_id += 1;

        Ok(res)
    }

    fn tick_animation(
        lua: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        animation: u32,
        delta_time: f64,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        if let Some(VgImage::Animation(anim)) = _vgfx
            .scoped_assets
            .get_mut(&lua.key())
            .ok_or(mlua::Error::external("Assets not initialized"))?
            .images
            .get_mut(&animation)
        {
            anim.tick(delta_time)
        }
        Ok(())
    }

    fn load_shared_texture(
        _lua_index: &LuaKey,
        _vgfx: &RefMut<Vgfx>,
        key: String,
        path: String,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        unimplemented()
    }

    fn load_shared_skin_texture(
        _vgfx: &RefMut<Vgfx>,
        key: String,
        path: String,
    ) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        unimplemented()
    }

    fn _get_shared_texture(_vgfx: &RefMut<Vgfx>, key: String) -> mlua::Result<()> {
        let mut _vgfx_lock = _vgfx.write().expect("Lock error");
        let _vgfx = _vgfx_lock.deref_mut();
        unimplemented()
    }

    fn create_shaded_mesh(
        lua: &LuaKey,
        context: &Arc<three_d::Context>,
        vgfx: &RefMut<Vgfx>,
        material: Option<String>,
        path: Option<String>,
    ) -> Result<ShadedMesh, mlua::Error> {
        let vgfx = vgfx.write().expect("Lock error");

        let mut shader_path = vgfx.game_folder.clone();
        shader_path.push("skins");
        shader_path.push(&vgfx.skin);
        shader_path.push("shaders");

        ShadedMesh::new(
            context,
            &material.unwrap_or_else(|| "guiTex".to_string()),
            path.map(PathBuf::from).unwrap_or(shader_path),
        )
        .map_err(mlua::Error::external)
    }

    const TEXT_ALIGN_BASELINE: u32 = TextAlign::ALIGN_BASELINE.bits(); // NVGalign::NVG_ALIGN_BASELINE
    const TEXT_ALIGN_BOTTOM: u32 = TextAlign::ALIGN_BOTTOM.bits(); // NVGalign::NVG_ALIGN_BOTTOM
    const TEXT_ALIGN_CENTER: u32 = TextAlign::ALIGN_CENTER.bits(); // NVGalign::NVG_ALIGN_CENTER
    const TEXT_ALIGN_LEFT: u32 = TextAlign::ALIGN_LEFT.bits(); // NVGalign::NVG_ALIGN_LEFT
    const TEXT_ALIGN_MIDDLE: u32 = TextAlign::ALIGN_MIDDLE.bits(); // NVGalign::NVG_ALIGN_MIDDLE
    const TEXT_ALIGN_RIGHT: u32 = TextAlign::ALIGN_RIGHT.bits(); // NVGalign::NVG_ALIGN_RIGHT
    const TEXT_ALIGN_TOP: u32 = TextAlign::ALIGN_TOP.bits(); // NVGalign::NVG_ALIGN_TOP
    const LINE_BEVEL: u8 = femtovg::LineJoin::Bevel as u8; // NVGlineCap::NVG_BEVEL
    const LINE_BUTT: u8 = femtovg::LineCap::Butt as u8; // NVGlineCap::NVG_BUTT
    const LINE_MITER: u8 = femtovg::LineJoin::Miter as u8; // NVGlineCap::NVG_MITER
    const LINE_ROUND: u8 = femtovg::LineCap::Round as u8; // NVGlineCap::NVG_ROUND
    const LINE_SQUARE: u8 = femtovg::LineCap::Square as u8; // NVGlineCap::NVG_SQUARE
    const IMAGE_GENERATE_MIPMAPS: u32 = ImageFlags::GENERATE_MIPMAPS.bits(); // NVGimageFlags::NVG_IMAGE_GENERATE_MIPMAPS
    const IMAGE_REPEATX: u32 = ImageFlags::REPEAT_X.bits(); // NVGimageFlags::NVG_IMAGE_REPEATX
    const IMAGE_REPEATY: u32 = ImageFlags::REPEAT_Y.bits(); // NVGimageFlags::NVG_IMAGE_REPEATY
    const IMAGE_FLIPY: u32 = ImageFlags::FLIP_Y.bits(); // NVGimageFlags::NVG_IMAGE_FLIPY
    const IMAGE_PREMULTIPLIED: u32 = ImageFlags::PREMULTIPLIED.bits(); // NVGimageFlags::NVG_IMAGE_PREMULTIPLIED
    const IMAGE_NEAREST: u32 = ImageFlags::NEAREST.bits(); // NVGimageFlags::NVG_IMAGE_NEAREST

    //Blend flags
    const BLEND_ZERO: u8 = femtovg::BlendFactor::Zero as u8;
    const BLEND_ONE: u8 = femtovg::BlendFactor::One as u8;
    const BLEND_SRC_COLOR: u8 = femtovg::BlendFactor::SrcColor as u8;
    const BLEND_ONE_MINUS_SRC_COLOR: u8 = femtovg::BlendFactor::OneMinusSrcColor as u8;
    const BLEND_DST_COLOR: u8 = femtovg::BlendFactor::DstColor as u8;
    const BLEND_ONE_MINUS_DST_COLOR: u8 = femtovg::BlendFactor::OneMinusDstColor as u8;
    const BLEND_SRC_ALPHA: u8 = femtovg::BlendFactor::SrcAlpha as u8;
    const BLEND_ONE_MINUS_SRC_ALPHA: u8 = femtovg::BlendFactor::OneMinusSrcAlpha as u8;
    const BLEND_DST_ALPHA: u8 = femtovg::BlendFactor::DstAlpha as u8;
    const BLEND_ONE_MINUS_DST_ALPHA: u8 = femtovg::BlendFactor::OneMinusDstAlpha as u8;
    const BLEND_SRC_ALPHA_SATURATE: u8 = femtovg::BlendFactor::SrcAlphaSaturate as u8;

    //Blend operations
    const BLEND_OP_SOURCE_OVER: u8 = femtovg::CompositeOperation::SourceOver as u8; //<<<<< default
    const BLEND_OP_SOURCE_IN: u8 = femtovg::CompositeOperation::SourceIn as u8;
    const BLEND_OP_SOURCE_OUT: u8 = femtovg::CompositeOperation::SourceOut as u8;
    const BLEND_OP_ATOP: u8 = femtovg::CompositeOperation::Atop as u8;
    const BLEND_OP_DESTINATION_OVER: u8 = femtovg::CompositeOperation::DestinationOver as u8;
    const BLEND_OP_DESTINATION_IN: u8 = femtovg::CompositeOperation::DestinationIn as u8;
    const BLEND_OP_DESTINATION_OUT: u8 = femtovg::CompositeOperation::DestinationOut as u8;
    const BLEND_OP_DESTINATION_ATOP: u8 = femtovg::CompositeOperation::DestinationAtop as u8;
    const BLEND_OP_LIGHTER: u8 = femtovg::CompositeOperation::Lighter as u8;
    const BLEND_OP_COPY: u8 = femtovg::CompositeOperation::Copy as u8;
    const BLEND_OP_XOR: u8 = femtovg::CompositeOperation::Xor as u8;
}

bitflags::bitflags! {
    struct TextAlign: u32 {
        // Horizontal align
        const ALIGN_LEFT = 1<<0; // Default, align text horizontally to left.
        const ALIGN_CENTER = 1<<1; // Align text horizontally to center.
        const ALIGN_RIGHT = 1<<2; // Align text horizontally to right.
        const HORIZONTAL = Self::ALIGN_LEFT.bits | Self::ALIGN_CENTER.bits | Self::ALIGN_RIGHT.bits;
        // Vertical align
        const ALIGN_TOP = 1<<3; // Align text vertically to top.
        const ALIGN_MIDDLE = 1<<4; // Align text vertically to middle.
        const ALIGN_BOTTOM = 1<<5; // Align text vertically to bottom.
        const ALIGN_BASELINE = 1<<6; // Default, align text vertically to baseline.
        const VERTICAL = Self::ALIGN_TOP.bits | Self::ALIGN_MIDDLE.bits | Self::ALIGN_BOTTOM.bits | Self::ALIGN_BASELINE.bits;
    }
}

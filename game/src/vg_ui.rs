use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

const COMPAT_TEXT_SCALE: f32 = 21.5 / 30.0; // Needed because old usc has two different text rendering methods for text, and fasttext/labels

use femtovg::{renderer::OpenGl, Canvas, Color, FontId, ImageFlags, ImageId, Paint, Path};


use log::warn;
use poll_promise::Promise;
use puffin::profile_scope;
use tealr::{
    mlu::{TealData, UserData, UserDataProxy},
    TypeName,
};

use tealr::mlu::mlua;
use three_d::FrameInput;

use crate::{
    animation::VgAnimation, config::GameConfig, help::add_lua_static_method,
    shaded_mesh::ShadedMesh, util::lua_address,
};

#[derive(Debug)]
enum VgImage {
    Static(ImageId),
    Animation(VgAnimation),
}

impl From<&VgImage> for ImageId {
    fn from(val: &VgImage) -> Self {
        match val {
            VgImage::Static(id) => *id,
            VgImage::Animation(anim) => anim.current_img_id(),
        }
    }
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
}

#[derive(UserData)]
pub struct Vgfx {
    pub canvas: Arc<Mutex<Canvas<OpenGl>>>,
    skin: String,
    restore_stack: Vec<VgfxPoint>,
    path: Option<Path>,
    fill_paint: Option<Paint>,
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
    font_size: f32,
    image_jobs: HashMap<String, Promise<image::DynamicImage>>,
    fallback_img: ImageId,
}

impl TypeName for Vgfx {
    fn get_type_parts() -> std::borrow::Cow<'static, [tealr::NamePart]> {
        use std::borrow::Cow;

        Cow::Borrowed(&[tealr::NamePart::Type(tealr::TealType {
            name: Cow::Borrowed("gfx"),
            type_kind: tealr::KindOfType::External,
            generics: None,
        })])
    }
}

#[derive(Clone, Debug)]
struct Label {
    text: String,
    size: i32,
    monospace: bool,
    font: FontId,
}

impl Vgfx {
    pub fn new(canvas: Arc<Mutex<Canvas<OpenGl>>>, game_folder: std::path::PathBuf) -> Self {
        let (fallback_img, default_fonts) = {
            let mut canvas = canvas.lock().unwrap();

            let mut font_dir = game_folder.clone();
            font_dir.push("fonts");
            let default_fonts = canvas
                .add_font_dir(&font_dir)
                .expect("Failed to load default fonts");
            font_dir.push("settings");
            _ = canvas
                .add_font_dir(&font_dir)
                .expect("Failed to load settings fonts");
            (
                canvas
                    .create_image(
                        femtovg::ImageSource::try_from(
                            &image::load_from_memory(include_bytes!("static_assets/missing.png"))
                                .unwrap(),
                        )
                        .unwrap(),
                        ImageFlags::empty(),
                    )
                    .unwrap(),
                default_fonts,
            )
        };

        let config = &GameConfig::get();

        Self {
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
            fallback_img,
            scoped_assets: Default::default(),
            image_tint: None,
            font_size: 12.0,
            label_color: Color::white(),
            label_font: *default_fonts.first().expect("No default font loaded"),
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
            )
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
            .unwrap()
            .images
            .insert(self.next_img_id, VgImage::Static(img));
        let result = self.next_img_id;
        self.next_img_id += 1;

        Ok(result)
    }

    pub fn delete_image(&mut self, image: u32, lua_index: usize) {
        if let Some(VgImage::Static(id)) = self.scoped_assets[&lua_index].images.get(&image) {
            let id = *id;
            self.with_canvas(|x| x.delete_image(id));
        }
    }

    pub fn skin_folder(&self) -> PathBuf {
        let mut res = self.game_folder.clone();
        res.push("skins");
        res.push(&self.skin);
        res
    }
}

impl TealData for Vgfx {
    fn add_methods<'lua, T: tealr::mlu::TealDataMethods<'lua, Self>>(methods: &mut T) {
        //BeginPath
        add_lua_static_method(methods, "BeginPath", |_, _vgfx, _: ()| {
            _vgfx.path = Some(Path::new());
            _vgfx.label_color = Color::white();

            Ok(())
        });

        //Rect
        tealr::mlu::create_named_parameters!(RectParams with
          x : f32,
          y : f32,
          w : f32,
          h : f32,

        );
        add_lua_static_method(methods, "Rect", |_, _vgfx, p: RectParams| {
            let RectParams { x, y, w, h } = p;
            match _vgfx.path.as_mut() {
                Some(p) => {
                    p.rect(x, y, w, h);
                    Ok(())
                }
                None => Err(mlua::Error::external("No path begun".to_string())),
            }
        });

        //FastRect
        tealr::mlu::create_named_parameters!(FastRectParams with
          x : f32,
          y : f32,
          w : f32,
          h : f32,

        );
        add_lua_static_method(methods, "FastRect", |_, _vgfx, p: FastRectParams| {
            let FastRectParams { x, y, w, h } = p;
            match _vgfx.path.as_mut() {
                Some(p) => {
                    p.rect(x, y, w, h);
                    Ok(())
                }
                None => Err(mlua::Error::external("No path begun".to_string())),
            }
        });

        //Fill
        add_lua_static_method(methods, "Fill", |_, _vgfx, _: ()| {
            match (_vgfx.path.as_mut(), _vgfx.fill_paint.as_ref()) {
                (Some(path), Some(paint)) => {
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
        });

        //FillColor
        tealr::mlu::create_named_parameters!(FillColorParams with
          r : u8,
          g : u8,
          b : u8,
          a : Option<u8>,

        );
        add_lua_static_method(methods, "FillColor", |_, _vgfx, p: FillColorParams| {
            let FillColorParams { r, g, b, a } = p;
            let color = Color::rgba(r, g, b, a.unwrap_or(255));
            _vgfx.label_color = color;
            if let Some(paint) = _vgfx.fill_paint.as_mut() {
                paint.set_color(color);
            } else {
                _vgfx.fill_paint = Some(Paint::color(color));
            }
            Ok(())
        });

        //CreateImage
        tealr::mlu::create_named_parameters!(CreateImageParams with
          filename : String,
          imageflags : u32,

        );
        add_lua_static_method(
            methods,
            "CreateImage",
            |lua, _vgfx, p: CreateImageParams| {
                let CreateImageParams {
                    filename,
                    imageflags,
                } = p;

                let img = _vgfx
                    .with_canvas(|canvas| {
                        canvas.load_image_file(
                            &filename,
                            ImageFlags::from_bits(imageflags).unwrap_or(ImageFlags::empty()),
                        )
                    })?
                    .map_err(mlua::Error::external)?;

                let this_id = _vgfx.next_img_id;
                _vgfx.next_img_id += 1;
                _vgfx
                    .scoped_assets
                    .get_mut(&lua_address(lua))
                    .unwrap()
                    .images
                    .insert(this_id, VgImage::Static(img));
                Ok(this_id)
            },
        );

        //CreateSkinImage
        tealr::mlu::create_named_parameters!(CreateSkinImageParams with
          filename : String,
          imageflags : u32,

        );
        add_lua_static_method(
            methods,
            "CreateSkinImage",
            |lua, _vgfx, p: CreateSkinImageParams| {
                let CreateSkinImageParams {
                    filename,
                    imageflags,
                } = p;

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
                    .get_mut(&lua_address(lua))
                    .unwrap()
                    .images
                    .insert(this_id, VgImage::Static(img));
                Ok(Some(this_id))
            },
        );

        //ImageRect
        tealr::mlu::create_named_parameters!(ImageRectParams with
          x : f32,
          y : f32,
          w : f32,
          h : f32,
          image : u32,
          alpha : f32,
          angle : f32,

        );
        add_lua_static_method(methods, "ImageRect", |lua, _vgfx, p: ImageRectParams| {
            let ImageRectParams {
                x,
                y,
                w,
                h,
                image,
                alpha,
                angle,
            } = p;

            if let Some(img_id) = _vgfx.scoped_assets[&lua_address(lua)].images.get(&image) {
                let img_id = img_id.into();
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
                            Paint::image_tint(
                                img_id,
                                0.0,
                                0.0,
                                img_w as f32,
                                img_h as f32,
                                0.0,
                                tint,
                            )
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
                        canvas.fill_path(&mut rect, &paint);
                    });
                })
            } else {
                Ok(())
            }
        });

        //Text
        tealr::mlu::create_named_parameters!(TextParams with
          s : Option<String>,
          x : f32,
          y : f32,

        );
        add_lua_static_method(methods, "Text", |_, _vgfx, p: TextParams| {
            let TextParams { s, x, y } = p;
            if s.is_none() {
                return Ok(());
            }
            match _vgfx.fill_paint.as_ref() {
                Some(fill_paint) => {
                    let canvas = &mut _vgfx
                        .canvas
                        .try_lock()
                        .map_err(|_| mlua::Error::external("Canvas in use".to_string()))?;

                    let _scale = canvas.transform().average_scale();

                    canvas
                        .fill_text(x, y, s.unwrap(), fill_paint)
                        .map_err(mlua::Error::external)?;
                    Ok(())
                }
                None => todo!(),
            }
        });

        //TextAlign
        tealr::mlu::create_named_parameters!(TextAlignParams with
          align : u32,

        );
        add_lua_static_method(methods, "TextAlign", |_, _vgfx, p: TextAlignParams| {
            let align = TextAlign::from_bits(p.align)
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

            _vgfx.stroke_paint.set_text_align(horizontal);
            _vgfx.stroke_paint.set_text_baseline(vertical);
            if let Some(text_paint) = _vgfx.fill_paint.as_mut() {
                text_paint.set_text_align(horizontal);
                text_paint.set_text_baseline(vertical);
            }
            Ok(())
        });

        //FontFace
        tealr::mlu::create_named_parameters!(FontFaceParams with
          s : String,

        );
        add_lua_static_method(methods, "FontFace", |_, _vgfx, p: FontFaceParams| {
            if let Some(font_id) = _vgfx.fonts.get(&p.s) {
                _vgfx.label_font = *font_id;
                if let Some(text_paint) = _vgfx.fill_paint.as_mut() {
                    text_paint.set_font(&[*font_id]);
                }
            } else {
                warn!("No loaded font named: {}", &p.s)
            }
            Ok(())
        });

        //FontSize
        tealr::mlu::create_named_parameters!(FontSizeParams with
          size : f32,

        );
        add_lua_static_method(methods, "FontSize", |_, _vgfx, p: FontSizeParams| {
            if let Some(text_paint) = _vgfx.fill_paint.as_mut() {
                text_paint.set_font_size(p.size * COMPAT_TEXT_SCALE);
            }
            Ok(())
        });

        //Translate
        tealr::mlu::create_named_parameters!(TranslateParams with
          x : f32,
          y : f32,

        );
        add_lua_static_method(methods, "Translate", |_, _vgfx, p: TranslateParams| {
            let TranslateParams { x, y } = p;
            _vgfx.with_canvas(|canvas| canvas.translate(x, y))?;
            Ok(())
        });

        //Scale
        tealr::mlu::create_named_parameters!(ScaleParams with
          x : f32,
          y : f32,

        );
        add_lua_static_method(methods, "Scale", |_, _vgfx, p: ScaleParams| {
            let ScaleParams { x, y } = p;
            _vgfx.with_canvas(|canvas| canvas.scale(x, y))?;
            Ok(())
        });

        //Rotate
        tealr::mlu::create_named_parameters!(RotateParams with
          angle : f32,

        );
        add_lua_static_method(methods, "Rotate", |_, _vgfx, p: RotateParams| {
            _vgfx.with_canvas(|canvas| canvas.rotate(p.angle))?;
            Ok(())
        });

        //ResetTransform
        add_lua_static_method(methods, "ResetTransform", |_, _vgfx, _: ()| {
            _vgfx.with_canvas(|canvas| canvas.reset_transform())?;
            Ok(())
        });

        //LoadFont
        tealr::mlu::create_named_parameters!(LoadFontParams with
          name : String,
          filename : Option<String>,

        );
        add_lua_static_method(methods, "LoadFont", |_, _vgfx, p: LoadFontParams| {
            let name = p.name;
            if let (Some(font_id), Some(paint)) =
                (_vgfx.fonts.get(&name), _vgfx.fill_paint.as_mut())
            {
                paint.set_font(&[*font_id]);
                _vgfx.label_font = *font_id;
            } else {
                let path = p.filename.unwrap_or_else(|| name.clone());
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
        });

        //LoadSkinFont
        tealr::mlu::create_named_parameters!(LoadSkinFontParams with
          name : String,
          filename : Option<String>,

        );
        add_lua_static_method(
            methods,
            "LoadSkinFont",
            |_, _vgfx, p: LoadSkinFontParams| {
                let name = p.name;
                if let (Some(font_id), Some(paint)) =
                    (_vgfx.fonts.get(&name), _vgfx.fill_paint.as_mut())
                {
                    paint.set_font(&[*font_id]);
                    _vgfx.label_font = *font_id;
                } else {
                    let path = p.filename.unwrap_or_else(|| name.clone());
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
            },
        );

        //FastText
        tealr::mlu::create_named_parameters!(FastTextParams with
          input_text : String,
          x : f32,
          y : f32,

        );
        add_lua_static_method(methods, "FastText", |_, _vgfx, p: FastTextParams| {
            let FastTextParams { input_text, x, y } = p;
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
        });

        //CreateLabel
        tealr::mlu::create_named_parameters!(CreateLabelParams with
          text : Option<String>,
          size : i32,
          monospace : bool,

        );
        add_lua_static_method(
            methods,
            "CreateLabel",
            |lua, _vgfx, p: CreateLabelParams| {
                let CreateLabelParams {
                    text,
                    size,
                    monospace,
                } = p;

                _vgfx
                    .scoped_assets
                    .get_mut(&lua_address(lua))
                    .unwrap()
                    .labels
                    .insert(
                        _vgfx.next_label_id,
                        Label {
                            text: text.unwrap_or_default(),
                            size,
                            monospace,
                            font: _vgfx.label_font,
                        },
                    );

                let id = _vgfx.next_label_id;
                _vgfx.next_label_id += 1;

                Ok(id)
            },
        );

        //DrawLabel
        tealr::mlu::create_named_parameters!(DrawLabelParams with
          label_id : u32,
          x : f32,
          y : f32,
          max_width : Option<f32>,

        );
        add_lua_static_method(methods, "DrawLabel", |lua, _vgfx, p: DrawLabelParams| {
            let DrawLabelParams {
                label_id,
                x,
                y,
                max_width,
            } = p;

            if let Some(label) = _vgfx.scoped_assets[&lua_address(lua)].labels.get(&label_id) {
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
                    .with_color(_vgfx.label_color);

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
        });

        //MoveTo
        tealr::mlu::create_named_parameters!(MoveToParams with
          x : f32,
          y : f32,

        );
        add_lua_static_method(methods, "MoveTo", |_lua_index, _vgfx, p: MoveToParams| {
            let MoveToParams { x, y } = p;
            if let Some(path) = _vgfx.path.as_mut() {
                path.move_to(x, y);
                Ok(())
            } else {
                Err(mlua::Error::external("No path started".to_string()))
            }
        });

        //LineTo
        tealr::mlu::create_named_parameters!(LineToParams with
          x : f32,
          y : f32,

        );
        add_lua_static_method(methods, "LineTo", |_lua_index, _vgfx, p: LineToParams| {
            let LineToParams { x, y } = p;
            if let Some(path) = _vgfx.path.as_mut() {
                path.line_to(x, y);
                Ok(())
            } else {
                Err(mlua::Error::external("No path started".to_string()))
            }
        });

        //BezierTo
        tealr::mlu::create_named_parameters!(BezierToParams with
          c_1x : f32,
          c_1y : f32,
          c_2x : f32,
          c_2y : f32,
          x : f32,
          y : f32,

        );
        add_lua_static_method(
            methods,
            "BezierTo",
            |_lua_index, _vgfx, p: BezierToParams| {
                let BezierToParams {
                    c_1x,
                    c_1y,
                    c_2x,
                    c_2y,
                    x,
                    y,
                } = p;
                if let Some(path) = _vgfx.path.as_mut() {
                    path.bezier_to(c_1x, c_1y, c_2x, c_2y, x, y);
                    Ok(())
                } else {
                    Err(mlua::Error::external("No path started".to_string()))
                }
            },
        );

        //QuadTo
        tealr::mlu::create_named_parameters!(QuadToParams with
          cx : f32,
          cy : f32,
          x : f32,
          y : f32,

        );
        add_lua_static_method(methods, "QuadTo", |_lua_index, _vgfx, p: QuadToParams| {
            let QuadToParams { cx, cy, x, y } = p;
            if let Some(path) = _vgfx.path.as_mut() {
                path.quad_to(cx, cy, x, y);
                Ok(())
            } else {
                Err(mlua::Error::external("No path started".to_string()))
            }
        });

        //ArcTo
        tealr::mlu::create_named_parameters!(ArcToParams with
          x_1 : f32,
          y_1 : f32,
          x_2 : f32,
          y_2 : f32,
          radius : f32,

        );
        add_lua_static_method(methods, "ArcTo", |_lua_index, _vgfx, p: ArcToParams| {
            let ArcToParams {
                x_1,
                y_1,
                x_2,
                y_2,
                radius,
            } = p;
            if let Some(path) = _vgfx.path.as_mut() {
                path.arc_to(x_1, y_1, x_2, y_2, radius);
                Ok(())
            } else {
                Err(mlua::Error::external("No path started".to_string()))
            }
        });

        //ClosePath
        add_lua_static_method(methods, "ClosePath", |_lua_index, _vgfx, _: ()| {
            if let Some(path) = _vgfx.path.as_mut() {
                path.close();
                Ok(())
            } else {
                Err(mlua::Error::external("No path started".to_string()))
            }
        });

        //MiterLimit
        tealr::mlu::create_named_parameters!(MiterLimitParams with
          limit : f32,

        );
        add_lua_static_method(
            methods,
            "MiterLimit",
            |_lua_index, _vgfx, p: MiterLimitParams| {
                _vgfx.stroke_paint.set_miter_limit(p.limit);
                Ok(())
            },
        );

        //StrokeWidth
        tealr::mlu::create_named_parameters!(StrokeWidthParams with
          size : f32,

        );
        add_lua_static_method(
            methods,
            "StrokeWidth",
            |_lua_index, _vgfx, p: StrokeWidthParams| {
                _vgfx.stroke_paint.set_line_width(p.size);
                Ok(())
            },
        );

        //LineCap
        tealr::mlu::create_named_parameters!(LineCapParams with
          cap : u8,

        );
        add_lua_static_method(methods, "LineCap", |_lua_index, _vgfx, p: LineCapParams| {
            _vgfx
                .stroke_paint
                .set_line_cap(unsafe { std::mem::transmute(p.cap) });
            Ok(())
        });

        //LineJoin
        tealr::mlu::create_named_parameters!(LineJoinParams with
          join : u8,

        );
        add_lua_static_method(
            methods,
            "LineJoin",
            |_lua_index, _vgfx, p: LineJoinParams| {
                _vgfx
                    .stroke_paint
                    .set_line_join(unsafe { std::mem::transmute(p.join) });
                Ok(())
            },
        );

        //Stroke
        add_lua_static_method(methods, "Stroke", |_lua_index, _vgfx, _: ()| {
            if let Some(path) = _vgfx.path.as_mut() {
                let canvas = &mut _vgfx
                    .canvas
                    .try_lock()
                    .map_err(|_| mlua::Error::external("Canvas in use".to_string()))?;
                canvas.stroke_path(path, &_vgfx.stroke_paint);
            }
            Ok(())
        });

        //StrokeColor
        tealr::mlu::create_named_parameters!(StrokeColorParams with
          r : u8,
          g : u8,
          b : u8,
          a : Option<u8>,

        );
        add_lua_static_method(
            methods,
            "StrokeColor",
            |_lua_index, _vgfx, p: StrokeColorParams| {
                let StrokeColorParams { r, g, b, a } = p;
                _vgfx
                    .stroke_paint
                    .set_color(Color::rgba(r, g, b, a.unwrap_or(255))); //TODO
                Ok(())
            },
        );

        //UpdateLabel
        tealr::mlu::create_named_parameters!(UpdateLabelParams with
          label_id : u32,
          text : String,
          size : i32,

        );
        add_lua_static_method(
            methods,
            "UpdateLabel",
            |lua, _vgfx, p: UpdateLabelParams| {
                if let Some(label) = _vgfx
                    .scoped_assets
                    .get_mut(&lua_address(lua))
                    .unwrap()
                    .labels
                    .get_mut(&p.label_id)
                {
                    label.text = p.text;
                    label.size = p.size;
                    label.font = _vgfx.label_font;
                    Ok(())
                } else {
                    Err(mlua::Error::external(format!(
                        "No label with id {}",
                        p.label_id
                    )))
                }
            },
        );

        //DrawGauge
        tealr::mlu::create_named_parameters!(DrawGaugeParams with
          rate : f32,
          x : f32,
          y : f32,
          w : f32,
          h : f32,
          delta_time : f32,

        );
        add_lua_static_method(
            methods,
            "DrawGauge",
            |_lua_index, _vgfx, _: DrawGaugeParams| -> Result<(), mlua::Error> {
                Err(mlua::Error::external("Function removed".to_string()))
            },
        );

        //SetGaugeColor
        tealr::mlu::create_named_parameters!(SetGaugeColorParams with
          colorindex : i32,
          r : i32,
          g : i32,
          b : i32,

        );
        add_lua_static_method(
            methods,
            "SetGaugeColor",
            |_lua_index, _vgfx, _: SetGaugeColorParams| -> Result<(), mlua::Error> {
                Err(mlua::Error::external("Function removed".to_string()))
            },
        );

        //RoundedRect
        tealr::mlu::create_named_parameters!(RoundedRectParams with
          x : f32,
          y : f32,
          w : f32,
          h : f32,
          r : f32,

        );
        add_lua_static_method(
            methods,
            "RoundedRect",
            |_lua_index, _vgfx, p: RoundedRectParams| {
                let RoundedRectParams { x, y, w, h, r } = p;
                if let Some(path) = _vgfx.path.as_mut() {
                    path.rounded_rect(x, y, w, h, r);
                    Ok(())
                } else {
                    Err(mlua::Error::external("No path started".to_string()))
                }
            },
        );

        //RoundedRectVarying
        tealr::mlu::create_named_parameters!(RoundedRectVaryingParams with
          x : f32,
          y : f32,
          w : f32,
          h : f32,
          rad_top_left : f32,
          rad_top_right : f32,
          rad_bottom_right : f32,
          rad_bottom_left : f32,

        );
        add_lua_static_method(
            methods,
            "RoundedRectVarying",
            |_lua_index, _vgfx, p: RoundedRectVaryingParams| {
                let RoundedRectVaryingParams {
                    x,
                    y,
                    w,
                    h,
                    rad_top_left,
                    rad_top_right,
                    rad_bottom_right,
                    rad_bottom_left,
                } = p;
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
            },
        );

        //Ellipse
        tealr::mlu::create_named_parameters!(EllipseParams with
          cx : f32,
          cy : f32,
          rx : f32,
          ry : f32,

        );
        add_lua_static_method(methods, "Ellipse", |_lua_index, _vgfx, p: EllipseParams| {
            let EllipseParams { cx, cy, rx, ry } = p;
            if let Some(path) = _vgfx.path.as_mut() {
                path.ellipse(cx, cy, rx, ry);
                Ok(())
            } else {
                Err(mlua::Error::external("No path started".to_string()))
            }
        });

        //Circle
        tealr::mlu::create_named_parameters!(CircleParams with
          cx : f32,
          cy : f32,
          r : f32,

        );
        add_lua_static_method(methods, "Circle", |_lua_index, _vgfx, p: CircleParams| {
            let CircleParams { cx, cy, r } = p;
            if let Some(path) = _vgfx.path.as_mut() {
                path.circle(cx, cy, r);
                Ok(())
            } else {
                Err(mlua::Error::external("No path started".to_string()))
            }
        });

        //SkewX
        tealr::mlu::create_named_parameters!(SkewXParams with
          angle : f32,

        );
        add_lua_static_method(methods, "SkewX", |_lua_index, _vgfx, p: SkewXParams| {
            _vgfx.with_canvas(|canvas| canvas.skew_x(p.angle))?;
            Ok(())
        });

        //SkewY
        tealr::mlu::create_named_parameters!(SkewYParams with
          angle : f32,

        );
        add_lua_static_method(methods, "SkewY", |_lua_index, _vgfx, p: SkewYParams| {
            _vgfx.with_canvas(|canvas| canvas.skew_y(p.angle))?;
            Ok(())
        });

        //LinearGradient
        tealr::mlu::create_named_parameters!(LinearGradientParams with
          sx : f32,
          sy : f32,
          ex : f32,
          ey : f32,

        );
        add_lua_static_method(
            methods,
            "LinearGradient",
            |lua, _vgfx, p: LinearGradientParams| {
                let LinearGradientParams { sx, sy, ex, ey } = p;

                _vgfx
                    .scoped_assets
                    .get_mut(&lua_address(lua))
                    .unwrap()
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
            },
        );

        //BoxGradient
        tealr::mlu::create_named_parameters!(BoxGradientParams with
          x : f32,
          y : f32,
          w : f32,
          h : f32,
          r : f32,
          f : f32,

        );
        add_lua_static_method(
            methods,
            "BoxGradient",
            |_lua_index, _vgfx, p: BoxGradientParams| {
                let BoxGradientParams { x, y, w, h, r, f } = p;
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
            },
        );

        //RadialGradient
        tealr::mlu::create_named_parameters!(RadialGradientParams with
          cx : f32,
          cy : f32,
          inr : f32,
          outr : f32,

        );
        add_lua_static_method(
            methods,
            "RadialGradient",
            |lua, _vgfx, p: RadialGradientParams| {
                let RadialGradientParams { cx, cy, inr, outr } = p;

                _vgfx
                    .scoped_assets
                    .get_mut(&lua_address(lua))
                    .unwrap()
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
            },
        );

        //ImagePattern
        tealr::mlu::create_named_parameters!(ImagePatternParams with
          ox : f32,
          oy : f32,
          ex : f32,
          ey : f32,
          angle : f32,
          image : u32,
          alpha : f32,

        );
        add_lua_static_method(
            methods,
            "ImagePattern",
            |lua, _vgfx, p: ImagePatternParams| {
                let ImagePatternParams {
                    ox,
                    oy,
                    ex,
                    ey,
                    angle,
                    image,
                    alpha,
                } = p;

                if let Some(id) = _vgfx.scoped_assets[&lua_address(lua)].images.get(&image) {
                    let id: ImageId = id.into();
                    let paint = Paint::image(id, ox, oy, ex, ey, angle, alpha);
                    _vgfx
                        .scoped_assets
                        .get_mut(&lua_address(lua))
                        .unwrap()
                        .paints
                        .insert(_vgfx.next_paint_id, paint);
                    let paint_id = _vgfx.next_paint_id;
                    _vgfx.next_paint_id += 1;
                    _vgfx
                        .scoped_assets
                        .get_mut(&lua_address(lua))
                        .unwrap()
                        .paint_imgs
                        .insert(paint_id, id);
                    Ok(paint_id)
                } else {
                    Err(mlua::Error::external(format!("No image with id {image}")))
                }
            },
        );

        //UpdateImagePattern
        tealr::mlu::create_named_parameters!(UpdateImagePatternParams with
          paint : u32,
          ox : f32,
          oy : f32,
          ex : f32,
          ey : f32,
          angle : f32,
          alpha : f32,
        );
        add_lua_static_method(
            methods,
            "UpdateImagePattern",
            |lua, _vgfx, p: UpdateImagePatternParams| {
                let UpdateImagePatternParams {
                    paint,
                    ox,
                    oy,
                    ex,
                    ey,
                    angle,
                    alpha,
                } = p;

                let assets = _vgfx.scoped_assets.get_mut(&lua_address(lua)).unwrap();
                if let (Some(pattern_paint), Some(img)) =
                    (assets.paints.get_mut(&paint), assets.paint_imgs.get(&paint))
                {
                    *pattern_paint = Paint::image(*img, ox, oy, ex, ey, angle, alpha);
                }
                Ok(())
            },
        );

        //GradientColors
        tealr::mlu::create_named_parameters!(GradientColorsParams with
          ri : i32,
          gi : i32,
          bi : i32,
          ai : i32,
          ro : i32,
          go : i32,
          bo : i32,
          ao : i32,

        );
        add_lua_static_method(
            methods,
            "GradientColors",
            |_lua_index, _vgfx, p: GradientColorsParams| {
                let GradientColorsParams {
                    ri,
                    gi,
                    bi,
                    ai,
                    ro,
                    go,
                    bo,
                    ao,
                } = p;
                _vgfx.gradient_colors = [
                    Color::rgba(ri as u8, gi as u8, bi as u8, ai as u8),
                    Color::rgba(ro as u8, go as u8, bo as u8, ao as u8),
                ];
                Ok(())
            },
        );

        //FillPaint
        tealr::mlu::create_named_parameters!(FillPaintParams with
          paint : u32,

        );
        add_lua_static_method(methods, "FillPaint", |lua, _vgfx, p: FillPaintParams| {
            if let Some(paint) = _vgfx.scoped_assets[&lua_address(lua)].paints.get(&p.paint) {
                _vgfx.fill_paint = Some(paint.clone());
            }
            Ok(())
        });

        //StrokePaint
        tealr::mlu::create_named_parameters!(StrokePaintParams with
          paint : u32,

        );
        add_lua_static_method(
            methods,
            "StrokePaint",
            |lua, _vgfx, p: StrokePaintParams| {
                if let Some(paint) = _vgfx.scoped_assets[&lua_address(lua)].paints.get(&p.paint) {
                    _vgfx.stroke_paint = paint.clone();
                }

                Ok(())
            },
        );

        //Save
        add_lua_static_method(methods, "Save", |_, _vgfx, _: ()| {
            _vgfx.with_canvas(|canvas| canvas.save())?;
            _vgfx.restore_stack.push(VgfxPoint {
                path: _vgfx.path.clone(),
                fill_paint: _vgfx.fill_paint.clone(),
                stroke_paint: _vgfx.stroke_paint.clone(),
            });
            //TODO: stacks for custom stuff
            Ok(())
        });

        //Restore
        add_lua_static_method(methods, "Restore", |_lua_index, _vgfx, _: ()| {
            _vgfx.with_canvas(|canvas| canvas.restore())?;

            if let Some(restore) = _vgfx.restore_stack.pop() {
                let VgfxPoint {
                    path,
                    fill_paint,
                    stroke_paint,
                } = restore;

                _vgfx.path = path;
                _vgfx.fill_paint = fill_paint;
                _vgfx.stroke_paint = stroke_paint;
            }

            //TODO: stacks for custom stuff
            Ok(())
        });

        //Reset
        add_lua_static_method(methods, "Reset", |_lua_index, _vgfx, _: ()| {
            _vgfx.restore_stack.clear();
            _vgfx.with_canvas(|canvas| canvas.reset())
        });

        //PathWinding
        tealr::mlu::create_named_parameters!(PathWindingParams with
          dir : i32,

        );
        add_lua_static_method(
            methods,
            "PathWinding",
            |_lua_index, _vgfx, _p: PathWindingParams| {
                todo!();
                Ok(0)
            },
        );

        //ForceRender
        add_lua_static_method(methods, "ForceRender", |_lua_index, _vgfx, _: ()| {
            //TODO: Flush game render as well
            //_vgfx.with_canvas(|canvas| canvas.flush())?;
            Ok(())
        });

        //LoadImageJob
        tealr::mlu::create_named_parameters!(LoadImageJobParams with
          path : String,
          placeholder : Option<u32>,
          w : Option<u32>,
          h : Option<u32>,

        );
        add_lua_static_method(
            methods,
            "LoadImageJob",
            |lua, _vgfx, p: LoadImageJobParams| {
                let LoadImageJobParams {
                    path,
                    placeholder,
                    w,
                    h,
                } = p;

                if let Some((key, job)) = _vgfx.image_jobs.remove_entry(&path) {
                    match job.try_take() {
                        Ok(img) if img.width() > 0 => {
                            let img_id = _vgfx.with_canvas(|c| {
                                c.create_image(
                                    femtovg::ImageSource::try_from(&img)
                                        .map_err(mlua::Error::external)?,
                                    ImageFlags::empty(),
                                )
                                .map_err(mlua::Error::external)
                            })??;

                            _vgfx
                                .scoped_assets
                                .get_mut(&lua_address(lua))
                                .unwrap()
                                .images
                                .insert(_vgfx.next_img_id, VgImage::Static(img_id));
                            _vgfx
                                .scoped_assets
                                .get_mut(&lua_address(lua))
                                .unwrap()
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
                if !_vgfx.scoped_assets[&lua_address(lua)]
                    .job_imgs
                    .contains_key(&path)
                {
                    _vgfx
                        .image_jobs
                        .entry(path.clone())
                        .or_insert_with(move || {
                            Promise::spawn_thread("load image", move || {
                                image::open(key)
                                    .map(|img| {
                                        if let (Some(w), Some(h)) = (w, h) {
                                            img.resize(
                                                w,
                                                h,
                                                image::imageops::FilterType::CatmullRom,
                                            )
                                        } else {
                                            img
                                        }
                                    })
                                    .unwrap_or_default()
                            })
                        });
                    _vgfx
                        .scoped_assets
                        .get_mut(&lua_address(lua))
                        .unwrap()
                        .job_imgs
                        .insert(path.clone(), placeholder.unwrap_or_default());
                }

                Ok(*_vgfx.scoped_assets[&lua_address(lua)]
                    .job_imgs
                    .get(&path)
                    .unwrap_or(&placeholder.unwrap_or_default()))
            },
        );

        //LoadWebImageJob
        tealr::mlu::create_named_parameters!(LoadWebImageJobParams with
          url : String,
          placeholder : i32,
          w : i32,
          h : i32,

        );
        add_lua_static_method(
            methods,
            "LoadWebImageJob",
            |_lua_index, _vgfx, _p: LoadWebImageJobParams| {
                todo!();
                Ok(0)
            },
        );

        //Scissor
        tealr::mlu::create_named_parameters!(ScissorParams with
          x : f32,
          y : f32,
          w : f32,
          h : f32,

        );
        add_lua_static_method(methods, "Scissor", |_lua_index, _vgfx, p: ScissorParams| {
            let ScissorParams { x, y, w, h } = p;
            _vgfx.with_canvas(|canvas| canvas.scissor(x, y, w, h))?;

            Ok(())
        });

        //IntersectScissor
        tealr::mlu::create_named_parameters!(IntersectScissorParams with
          x : f32,
          y : f32,
          w : f32,
          h : f32,

        );
        add_lua_static_method(
            methods,
            "IntersectScissor",
            |_lua_index, _vgfx, p: IntersectScissorParams| {
                let IntersectScissorParams { x, y, w, h } = p;
                _vgfx.with_canvas(|canvas| canvas.intersect_scissor(x, y, w, h))?;
                Ok(())
            },
        );

        //ResetScissor
        add_lua_static_method(methods, "ResetScissor", |_lua_index, _vgfx, _: ()| {
            _vgfx.with_canvas(|canvas| canvas.reset_scissor())?;
            Ok(())
        });

        //TextBounds
        tealr::mlu::create_named_parameters!(TextBoundsParams with
          x : f32,
          y : f32,
          s : String,

        );
        add_lua_static_method(
            methods,
            "TextBounds",
            |_lua_index, _vgfx, p: TextBoundsParams| {
                let TextBoundsParams { x, y, s } = p;

                if let Some(paint) = _vgfx.fill_paint.as_ref() {
                    let canvas = &mut _vgfx
                        .canvas
                        .try_lock()
                        .map_err(|_| mlua::Error::external("Canvas in use".to_string()))?;

                    let bounds = canvas
                        .measure_text(x, y, s, paint)
                        .map_err(mlua::Error::external)?;
                    Ok((x, y, x + bounds.width(), y + bounds.height()))
                } else {
                    Err(mlua::Error::external("No text paint set".to_string()))
                }
            },
        );

        //LabelSize
        tealr::mlu::create_named_parameters!(LabelSizeParams with
          label : u32,

        );
        add_lua_static_method(methods, "LabelSize", |lua, _vgfx, p: LabelSizeParams| {
            let mut paint = _vgfx
                .fill_paint
                .clone()
                .unwrap_or_else(|| _vgfx.stroke_paint.clone());

            let canvas = _vgfx.canvas.lock().unwrap();
            if let Some(label) = _vgfx.scoped_assets[&lua_address(lua)].labels.get(&p.label) {
                paint.set_font(&[label.font]);
                paint.set_font_size(label.size as f32);
                let size = canvas
                    .measure_text(0.0, 0.0, &label.text, &paint)
                    .map_err(mlua::Error::external)?;
                Ok((size.width(), size.height()))
            } else {
                Err(mlua::Error::RuntimeError(format!(
                    "No label with id: {}",
                    p.label
                )))
            }
        });

        //FastTextSize
        tealr::mlu::create_named_parameters!(FastTextSizeParams with
          text : String,

        );
        add_lua_static_method(
            methods,
            "FastTextSize",
            |_lua_index, _vgfx, _p: FastTextSizeParams| {
                todo!();
                Ok(0)
            },
        );

        //ImageSize
        tealr::mlu::create_named_parameters!(ImageSizeParams with
          image : u32,

        );
        add_lua_static_method(methods, "ImageSize", |lua, _vgfx, p: ImageSizeParams| {
            if let Some(id) = _vgfx.scoped_assets[&lua_address(lua)].images.get(&p.image) {
                let id: ImageId = id.into();
                _vgfx
                    .with_canvas(|canvas| canvas.image_size(id))?
                    .map_err(mlua::Error::external)
            } else {
                Err(mlua::Error::external(format!(
                    "No image with id {}",
                    p.image
                )))
            }
        });

        //Arc
        tealr::mlu::create_named_parameters!(ArcParams with
          cx : f32,
          cy : f32,
          r : f32,
          a_0 : f32,
          a_1 : f32,
          dir : i32,

        );
        add_lua_static_method(methods, "Arc", |_lua_index, _vgfx, p: ArcParams| {
            let ArcParams {
                cx,
                cy,
                r,
                a_0,
                a_1,
                dir,
            } = p;
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
        });

        //SetImageTint
        tealr::mlu::create_named_parameters!(SetImageTintParams with
          r : u8,
          g : u8,
          b : u8,

        );
        add_lua_static_method(
            methods,
            "SetImageTint",
            |_lua_index, _vgfx, _p: SetImageTintParams| {
                let SetImageTintParams { r, g, b } = _p;
                if let Some(_paint) = _vgfx.fill_paint.as_mut() {
                    _vgfx.image_tint = Some(Color::rgb(r, g, b));
                }
                Ok(0)
            },
        );

        //GlobalCompositeOperation
        tealr::mlu::create_named_parameters!(GlobalCompositeOperationParams with
          op : u8,

        );
        add_lua_static_method(
            methods,
            "GlobalCompositeOperation",
            |_lua_index, _vgfx, p: GlobalCompositeOperationParams| {
                if p.op <= femtovg::CompositeOperation::Xor as u8 {
                    unsafe {
                        _vgfx.with_canvas(|canvas| {
                            canvas.global_composite_operation(std::mem::transmute(p.op))
                        })?
                    }
                }

                Ok(())
            },
        );

        //GlobalCompositeBlendFunc
        tealr::mlu::create_named_parameters!(GlobalCompositeBlendFuncParams with
          sfactor : u8,
          dfactor : u8,

        );
        add_lua_static_method(
            methods,
            "GlobalCompositeBlendFunc",
            |_lua_index, _vgfx, p: GlobalCompositeBlendFuncParams| {
                let last_factor = femtovg::BlendFactor::SrcAlphaSaturate as u8;
                if p.dfactor <= last_factor && p.sfactor <= last_factor {
                    unsafe {
                        _vgfx.with_canvas(|canvas| {
                            canvas.global_composite_blend_func(
                                std::mem::transmute(p.sfactor),
                                std::mem::transmute(p.dfactor),
                            )
                        })?
                    }
                }

                Ok(())
            },
        );

        //GlobalCompositeBlendFuncSeparate
        tealr::mlu::create_named_parameters!(GlobalCompositeBlendFuncSeparateParams with
          src_rgb : i32,
          dst_rgb : i32,
          src_alpha : i32,
          dst_alpha : i32,

        );
        add_lua_static_method(
            methods,
            "GlobalCompositeBlendFuncSeparate",
            |_lua_index, _vgfx, _p: GlobalCompositeBlendFuncSeparateParams| {
                todo!();
                Ok(0)
            },
        );

        //LoadAnimation
        tealr::mlu::create_named_parameters!(LoadAnimationParams with
          path : String,
          frametime : f64,
          loopcount : Option<usize>,
          compressed : Option<bool>,

        );
        add_lua_static_method(
            methods,
            "LoadAnimation",
            |lua, _vgfx, p: LoadAnimationParams| {
                let LoadAnimationParams {
                    path,
                    frametime,
                    loopcount,
                    compressed,
                } = p;

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
                    .get_mut(&lua_address(lua))
                    .unwrap()
                    .images
                    .insert(_vgfx.next_img_id, VgImage::Animation(anim));
                let res = _vgfx.next_img_id;
                _vgfx.next_img_id += 1;

                Ok(res)
            },
        );

        //GlobalAlpha
        tealr::mlu::create_named_parameters!(GlobalAlphaParams with
          alpha : f32,

        );
        add_lua_static_method(
            methods,
            "GlobalAlpha",
            |_lua_index, _vgfx, p: GlobalAlphaParams| {
                // todo!();
                _vgfx.canvas.lock().unwrap().set_global_alpha(p.alpha);
                Ok(())
            },
        );

        //LoadSkinAnimation
        add_lua_static_method(
            methods,
            "LoadSkinAnimation",
            |lua, _vgfx, p: LoadAnimationParams| {
                let LoadAnimationParams {
                    path,
                    frametime,
                    loopcount,
                    compressed,
                } = p;

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
                    .get_mut(&lua_address(lua))
                    .unwrap()
                    .images
                    .insert(_vgfx.next_img_id, VgImage::Animation(anim));
                let res = _vgfx.next_img_id;
                _vgfx.next_img_id += 1;

                Ok(res)
            },
        );

        //TickAnimation
        tealr::mlu::create_named_parameters!(TickAnimationParams with
          animation : u32,
          delta_time : f64,

        );
        add_lua_static_method(
            methods,
            "TickAnimation",
            |lua, _vgfx, p: TickAnimationParams| {
                let TickAnimationParams {
                    animation,
                    delta_time,
                } = p;

                if let Some(VgImage::Animation(anim)) = _vgfx
                    .scoped_assets
                    .get_mut(&lua_address(lua))
                    .unwrap()
                    .images
                    .get_mut(&animation)
                {
                    anim.tick(delta_time)
                }
                Ok(())
            },
        );

        //LoadSharedTexture
        tealr::mlu::create_named_parameters!(LoadSharedTextureParams with
          key : String,
          path : String,

        );
        add_lua_static_method(
            methods,
            "LoadSharedTexture",
            |_lua_index, _vgfx, _p: LoadSharedTextureParams| {
                todo!();
                Ok(0)
            },
        );

        //LoadSharedSkinTexture
        tealr::mlu::create_named_parameters!(LoadSharedSkinTextureParams with
          key : String,
          path : String,

        );
        add_lua_static_method(
            methods,
            "LoadSharedSkinTexture",
            |_, _vgfx, _p: LoadSharedSkinTextureParams| {
                todo!();
                Ok(0)
            },
        );

        //GetSharedTexture
        tealr::mlu::create_named_parameters!(GetSharedTextureParams with
          key : String,

        );
        add_lua_static_method(
            methods,
            "GetSharedTexture",
            |_, _vgfx, _p: GetSharedTextureParams| {
                todo!();
                Ok(0)
            },
        );

        tealr::mlu::create_named_parameters!(CreateShadedMeshParams with
            material: Option<String>,
            path: Option<String>,
        );

        methods.add_function_mut("CreateShadedMesh", |lua, p: CreateShadedMeshParams| {
            let context = &lua.app_data_ref::<FrameInput<()>>().unwrap().context;
            let vgfx = &lua.app_data_ref::<Arc<Mutex<Vgfx>>>().unwrap();
            let vgfx = vgfx.lock().unwrap();

            let mut shader_path = vgfx.game_folder.clone();
            shader_path.push("skins");
            shader_path.push(&vgfx.skin);
            shader_path.push("shaders");

            ShadedMesh::new(
                context,
                &p.material.unwrap_or_else(|| "guiTex".to_string()),
                p.path.map(PathBuf::from).unwrap_or(shader_path),
            )
            .map_err(mlua::Error::external)
        })
    }
    fn add_fields<'lua, F: tealr::mlu::TealDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_function_get("TEXT_ALIGN_BASELINE", |_, _| {
            Ok(TextAlign::ALIGN_BASELINE.bits())
        }); // NVGalign::NVG_ALIGN_BASELINE
        fields.add_field_function_get("TEXT_ALIGN_BOTTOM", |_, _| {
            Ok(TextAlign::ALIGN_BOTTOM.bits())
        }); // NVGalign::NVG_ALIGN_BOTTOM
        fields.add_field_function_get("TEXT_ALIGN_CENTER", |_, _| {
            Ok(TextAlign::ALIGN_CENTER.bits())
        }); // NVGalign::NVG_ALIGN_CENTER
        fields.add_field_function_get("TEXT_ALIGN_LEFT", |_, _| Ok(TextAlign::ALIGN_LEFT.bits())); // NVGalign::NVG_ALIGN_LEFT
        fields.add_field_function_get("TEXT_ALIGN_MIDDLE", |_, _| {
            Ok(TextAlign::ALIGN_MIDDLE.bits())
        }); // NVGalign::NVG_ALIGN_MIDDLE
        fields.add_field_function_get("TEXT_ALIGN_RIGHT", |_, _| Ok(TextAlign::ALIGN_RIGHT.bits())); // NVGalign::NVG_ALIGN_RIGHT
        fields.add_field_function_get("TEXT_ALIGN_TOP", |_, _| Ok(TextAlign::ALIGN_TOP.bits())); // NVGalign::NVG_ALIGN_TOP
        fields.add_field_function_get("LINE_BEVEL", |_, _| Ok(femtovg::LineJoin::Bevel as u8)); // NVGlineCap::NVG_BEVEL
        fields.add_field_function_get("LINE_BUTT", |_, _| Ok(femtovg::LineCap::Butt as u8)); // NVGlineCap::NVG_BUTT
        fields.add_field_function_get("LINE_MITER", |_, _| Ok(femtovg::LineJoin::Miter as u8)); // NVGlineCap::NVG_MITER
        fields.add_field_function_get("LINE_ROUND", |_, _| Ok(femtovg::LineCap::Round as u8)); // NVGlineCap::NVG_ROUND
        fields.add_field_function_get("LINE_SQUARE", |_, _| Ok(femtovg::LineCap::Square as u8)); // NVGlineCap::NVG_SQUARE
        fields.add_field_function_get("IMAGE_GENERATE_MIPMAPS", |_, _| {
            Ok(ImageFlags::GENERATE_MIPMAPS.bits())
        }); // NVGimageFlags::NVG_IMAGE_GENERATE_MIPMAPS
        fields.add_field_function_get("IMAGE_REPEATX", |_, _| Ok(ImageFlags::REPEAT_X.bits())); // NVGimageFlags::NVG_IMAGE_REPEATX
        fields.add_field_function_get("IMAGE_REPEATY", |_, _| Ok(ImageFlags::REPEAT_Y.bits())); // NVGimageFlags::NVG_IMAGE_REPEATY
        fields.add_field_function_get("IMAGE_FLIPY", |_, _| Ok(ImageFlags::FLIP_Y.bits())); // NVGimageFlags::NVG_IMAGE_FLIPY
        fields.add_field_function_get("IMAGE_PREMULTIPLIED", |_, _| {
            Ok(ImageFlags::PREMULTIPLIED.bits())
        }); // NVGimageFlags::NVG_IMAGE_PREMULTIPLIED
        fields.add_field_function_get("IMAGE_NEAREST", |_, _| Ok(ImageFlags::NEAREST.bits()));
        // NVGimageFlags::NVG_IMAGE_NEAREST

        //Blend flags
        fields.add_field_function_get("BLEND_ZERO", |_, _| Ok(femtovg::BlendFactor::Zero as u8));
        fields.add_field_function_get("BLEND_ONE", |_, _| Ok(femtovg::BlendFactor::One as u8));
        fields.add_field_function_get("BLEND_SRC_COLOR", |_, _| {
            Ok(femtovg::BlendFactor::SrcColor as u8)
        });
        fields.add_field_function_get("BLEND_ONE_MINUS_SRC_COLOR", |_, _| {
            Ok(femtovg::BlendFactor::OneMinusSrcColor as u8)
        });
        fields.add_field_function_get("BLEND_DST_COLOR", |_, _| {
            Ok(femtovg::BlendFactor::DstColor as u8)
        });
        fields.add_field_function_get("BLEND_ONE_MINUS_DST_COLOR", |_, _| {
            Ok(femtovg::BlendFactor::OneMinusDstColor as u8)
        });
        fields.add_field_function_get("BLEND_SRC_ALPHA", |_, _| {
            Ok(femtovg::BlendFactor::SrcAlpha as u8)
        });
        fields.add_field_function_get("BLEND_ONE_MINUS_SRC_ALPHA", |_, _| {
            Ok(femtovg::BlendFactor::OneMinusSrcAlpha as u8)
        });
        fields.add_field_function_get("BLEND_DST_ALPHA", |_, _| {
            Ok(femtovg::BlendFactor::DstAlpha as u8)
        });
        fields.add_field_function_get("BLEND_ONE_MINUS_DST_ALPHA", |_, _| {
            Ok(femtovg::BlendFactor::OneMinusDstAlpha as u8)
        });
        fields.add_field_function_get("BLEND_SRC_ALPHA_SATURATE", |_, _| {
            Ok(femtovg::BlendFactor::SrcAlphaSaturate as u8)
        });

        //Blend operations
        fields.add_field_function_get("BLEND_OP_SOURCE_OVER", |_, _| {
            Ok(femtovg::CompositeOperation::SourceOver as u8)
        }); //<<<<< default
        fields.add_field_function_get("BLEND_OP_SOURCE_IN", |_, _| {
            Ok(femtovg::CompositeOperation::SourceIn as u8)
        });
        fields.add_field_function_get("BLEND_OP_SOURCE_OUT", |_, _| {
            Ok(femtovg::CompositeOperation::SourceOut as u8)
        });
        fields.add_field_function_get("BLEND_OP_ATOP", |_, _| {
            Ok(femtovg::CompositeOperation::Atop as u8)
        });
        fields.add_field_function_get("BLEND_OP_DESTINATION_OVER", |_, _| {
            Ok(femtovg::CompositeOperation::DestinationOver as u8)
        });
        fields.add_field_function_get("BLEND_OP_DESTINATION_IN", |_, _| {
            Ok(femtovg::CompositeOperation::DestinationIn as u8)
        });
        fields.add_field_function_get("BLEND_OP_DESTINATION_OUT", |_, _| {
            Ok(femtovg::CompositeOperation::DestinationOut as u8)
        });
        fields.add_field_function_get("BLEND_OP_DESTINATION_ATOP", |_, _| {
            Ok(femtovg::CompositeOperation::DestinationAtop as u8)
        });
        fields.add_field_function_get("BLEND_OP_LIGHTER", |_, _| {
            Ok(femtovg::CompositeOperation::Lighter as u8)
        });
        fields.add_field_function_get("BLEND_OP_COPY", |_, _| {
            Ok(femtovg::CompositeOperation::Copy as u8)
        });
        fields.add_field_function_get("BLEND_OP_XOR", |_, _| {
            Ok(femtovg::CompositeOperation::Xor as u8)
        });
    }
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

// document and expose the global proxy
#[derive(Default)]
pub struct ExportVgfx;
impl tealr::mlu::ExportInstances for ExportVgfx {
    fn add_instances<'lua, T: tealr::mlu::InstanceCollector<'lua>>(
        self,
        instance_collector: &mut T,
    ) -> mlua::Result<()> {
        instance_collector.document_instance("Documentation for the exposed static proxy");

        // note that the proxy type is NOT `Example` but a special mlua type, which is represented differnetly in .d.tl as well
        instance_collector.add_instance("gfx", UserDataProxy::<Vgfx>::new)?;
        Ok(())
    }
}

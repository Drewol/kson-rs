use std::{
    borrow::BorrowMut,
    cell::RefCell,
    collections::HashMap,
    sync::{Arc, Mutex},
};

use femtovg::{renderer::OpenGl, Canvas, Color, FontId, ImageFlags, ImageId, Paint, Path};
use once_cell::unsync::OnceCell;
use tealr::{
    mlu::{TealData, UserData, UserDataProxy},
    TypeName,
};

use tealr::mlu::mlua;

#[derive(UserData, Clone)]
pub struct Vgfx {
    pub canvas: Arc<Mutex<Canvas<OpenGl>>>,
    skin: String,
    path: Option<Path>,
    fill_paint: Option<Paint>,
    stroke_paint: Paint,
    gradient_colors: [Color; 2],
    game_folder: std::path::PathBuf,
    next_img_id: u32,
    next_paint_id: u32,
    next_label_id: u32,
    images: HashMap<u32, ImageId>,
    paints: HashMap<u32, Paint>,
    labels: HashMap<u32, Label>,
    fonts: HashMap<String, FontId>,
    paint_imgs: HashMap<u32, ImageId>,
    current_font: Option<FontId>,
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

thread_local! {
    static INSTANCE: RefCell<Option<Vgfx>> = RefCell::new(None);
}

pub fn init_vgfx(canvas: Arc<Mutex<Canvas<OpenGl>>>, game_folder: std::path::PathBuf) {
    INSTANCE.with(|a| {
        a.replace(Some(Vgfx::new(canvas, game_folder)));
    })
}

pub fn with_vgfx(f: impl FnMut(&mut Vgfx)) {
    INSTANCE.with(|instance| {
        let mut borrowed = instance.borrow_mut();
        borrowed.as_mut().map(f);
    })
}

#[derive(Clone)]
struct Label {
    text: String,
    size: i32,
    monospace: bool,
}

impl Vgfx {
    pub fn new(canvas: Arc<Mutex<Canvas<OpenGl>>>, game_folder: std::path::PathBuf) -> Self {
        {
            let mut canvas_lock = canvas.try_lock();
            if let Ok(canvas) = canvas_lock.borrow_mut() {
                let mut font_dir = game_folder.clone();
                font_dir.push("fonts");
                _ = canvas.add_font_dir(&font_dir);
                font_dir.push("settings");
                _ = canvas.add_font_dir(&font_dir);
            }
        }

        Self {
            canvas,
            game_folder,
            skin: "Default".to_string(),
            path: None,
            fill_paint: None,
            stroke_paint: Paint::color(Color::white()),
            gradient_colors: [Color::black(), Color::black()],
            images: Default::default(),
            paints: Default::default(),
            labels: Default::default(),
            fonts: Default::default(),
            paint_imgs: Default::default(),
            next_img_id: 1,
            next_paint_id: 1,
            next_label_id: 1,
            current_font: None,
        }
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
}

impl TealData for Vgfx {
    fn add_methods<'lua, T: tealr::mlu::TealDataMethods<'lua, Self>>(methods: &mut T) {
        //BeginPath
        methods.add_method_mut("BeginPath", |_, _vgfx, _: ()| {
            _vgfx.path = Some(Path::new());
            Ok(())
        });

        //Rect
        tealr::mlu::create_named_parameters!(RectParams with
          x : f32,
          y : f32,
          w : f32,
          h : f32,

        );
        methods.add_method_mut("Rect", |_, _vgfx, p: RectParams| {
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
        methods.add_method_mut("FastRect", |_, _vgfx, p: FastRectParams| {
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
        methods.add_method_mut("Fill", |_, _vgfx, _: ()| {
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
        methods.add_method_mut("FillColor", |_, _vgfx, p: FillColorParams| {
            let FillColorParams { r, g, b, a } = p;
            let color = Color::rgba(r, g, b, a.unwrap_or(255));
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
        methods.add_method_mut("CreateImage", |_, _vgfx, p: CreateImageParams| {
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
            _vgfx.images.insert(this_id, img);
            Ok(this_id)
        });

        //CreateSkinImage
        tealr::mlu::create_named_parameters!(CreateSkinImageParams with
          filename : String,
          imageflags : u32,

        );
        methods.add_method_mut("CreateSkinImage", |_, _vgfx, p: CreateSkinImageParams| {
            let CreateSkinImageParams {
                filename,
                imageflags,
            } = p;
            let mut path = _vgfx.game_folder.clone();
            path.push("skins");
            path.push(&_vgfx.skin);
            path.push("textures");
            path.push(filename);
            let img = _vgfx
                .with_canvas(|canvas| {
                    canvas.load_image_file(
                        &path,
                        ImageFlags::from_bits(imageflags).unwrap_or(ImageFlags::empty()),
                    )
                })?
                .map_err(mlua::Error::external)?;

            let this_id = _vgfx.next_img_id;
            _vgfx.next_img_id += 1;
            _vgfx.images.insert(this_id, img);
            Ok(this_id)
        });

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
        methods.add_method_mut("ImageRect", |_, _vgfx, p: ImageRectParams| {
            let ImageRectParams {
                x,
                y,
                w,
                h,
                image,
                alpha,
                angle,
            } = p;
            if let Some(img_id) = _vgfx.images.get(&image).cloned() {
                _vgfx.with_canvas(|canvas| {
                    let (img_w, img_h) =
                        canvas.image_size(img_id).map_err(mlua::Error::external)?;
                    let prev_transform = canvas.transform();
                    let scale_x = w / img_w as f32;
                    let scale_y = h / img_h as f32;
                    canvas.translate(x, y);
                    canvas.rotate(angle);
                    canvas.scale(scale_x, scale_y);
                    let paint = Paint::image_tint(
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
                    ); //TODO: Set from tint
                    let mut rect = Path::new();
                    rect.rect(0.0, 0.0, img_w as f32, img_h as f32);
                    canvas.fill_path(&mut rect, &paint);
                    canvas.set_transform(
                        prev_transform.0[0],
                        prev_transform.0[1],
                        prev_transform.0[2],
                        prev_transform.0[3],
                        prev_transform.0[4],
                        prev_transform.0[5],
                    );

                    Ok(())
                })?
            } else {
                Ok(())
            }
        });

        //Text
        tealr::mlu::create_named_parameters!(TextParams with
          s : String,
          x : f32,
          y : f32,

        );
        methods.add_method_mut("Text", |_, _vgfx, p: TextParams| {
            let TextParams { s, x, y } = p;
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
        });

        //TextAlign
        tealr::mlu::create_named_parameters!(TextAlignParams with
          align : u32,

        );
        methods.add_method_mut("TextAlign", |_, _vgfx, p: TextAlignParams| {
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
        methods.add_method_mut("FontFace", |_, _vgfx, p: FontFaceParams| {
            if let Some(font_id) = _vgfx.fonts.get(&p.s) {
                if let Some(text_paint) = _vgfx.fill_paint.as_mut() {
                    text_paint.set_font(&[*font_id]);
                }
            }
            Ok(())
        });

        //FontSize
        tealr::mlu::create_named_parameters!(FontSizeParams with
          size : f32,

        );
        methods.add_method_mut("FontSize", |_, _vgfx, p: FontSizeParams| {
            if let Some(text_paint) = _vgfx.fill_paint.as_mut() {
                text_paint.set_font_size(p.size);
            }
            Ok(())
        });

        //Translate
        tealr::mlu::create_named_parameters!(TranslateParams with
          x : f32,
          y : f32,

        );
        methods.add_method_mut("Translate", |_, _vgfx, p: TranslateParams| {
            let TranslateParams { x, y } = p;
            _vgfx.with_canvas(|canvas| canvas.translate(x, y))?;
            Ok(())
        });

        //Scale
        tealr::mlu::create_named_parameters!(ScaleParams with
          x : f32,
          y : f32,

        );
        methods.add_method_mut("Scale", |_, _vgfx, p: ScaleParams| {
            let ScaleParams { x, y } = p;
            _vgfx.with_canvas(|canvas| canvas.scale(x, y))?;
            Ok(())
        });

        //Rotate
        tealr::mlu::create_named_parameters!(RotateParams with
          angle : f32,

        );
        methods.add_method_mut("Rotate", |_, _vgfx, p: RotateParams| {
            _vgfx.with_canvas(|canvas| canvas.rotate(p.angle))?;
            Ok(())
        });

        //ResetTransform
        methods.add_method_mut("ResetTransform", |_, _vgfx, _: ()| {
            _vgfx.with_canvas(|canvas| canvas.reset_transform())?;
            Ok(())
        });

        //LoadFont
        tealr::mlu::create_named_parameters!(LoadFontParams with
          name : String,
          filename : Option<String>,

        );
        methods.add_method_mut("LoadFont", |_, _vgfx, p: LoadFontParams| {
            let name = p.name;
            if let Some(font_id) = _vgfx.fonts.get(&name) {
                _vgfx.current_font = Some(*font_id);
            } else {
                let path = p.filename.unwrap_or_else(|| name.clone());
                let font_id = _vgfx
                    .with_canvas(|canvas| canvas.add_font(&path))?
                    .map_err(mlua::Error::external)?;
                _vgfx.current_font = Some(font_id);
                _vgfx.fonts.insert(name, font_id);
            }

            Ok(())
        });

        //LoadSkinFont
        tealr::mlu::create_named_parameters!(LoadSkinFontParams with
          name : String,
          filename : Option<String>,

        );
        methods.add_method_mut("LoadSkinFont", |_, _vgfx, p: LoadSkinFontParams| {
            let name = p.name;
            if let Some(font_id) = _vgfx.fonts.get(&name) {
                _vgfx.current_font = Some(*font_id);
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
                _vgfx.current_font = Some(font_id);
                _vgfx.fonts.insert(name, font_id);
            }

            Ok(())
        });

        //FastText
        tealr::mlu::create_named_parameters!(FastTextParams with
          input_text : String,
          x : f32,
          y : f32,

        );
        methods.add_method_mut("FastText", |_, _vgfx, p: FastTextParams| {
            let FastTextParams { input_text, x, y } = p;
            match _vgfx.fill_paint.as_ref() {
                Some(fill_paint) => {
                    let canvas = &mut _vgfx
                        .canvas
                        .try_lock()
                        .map_err(|_| mlua::Error::external("Canvas in use".to_string()))?;
                    canvas
                        .fill_text(x, y, input_text, fill_paint)
                        .map_err(mlua::Error::external)?;
                    Ok(())
                }
                None => todo!(),
            }
        });

        //CreateLabel
        tealr::mlu::create_named_parameters!(CreateLabelParams with
          text : String,
          size : i32,
          monospace : bool,

        );
        methods.add_method_mut("CreateLabel", |_, _vgfx, p: CreateLabelParams| {
            let CreateLabelParams {
                text,
                size,
                monospace,
            } = p;

            _vgfx.labels.insert(
                _vgfx.next_label_id,
                Label {
                    text,
                    size,
                    monospace,
                },
            );

            let id = _vgfx.next_label_id;
            _vgfx.next_label_id += 1;

            Ok(id)
        });

        //DrawLabel
        tealr::mlu::create_named_parameters!(DrawLabelParams with
          label_id : u32,
          x : f32,
          y : f32,
          max_width : f32,

        );
        methods.add_method_mut("DrawLabel", |_, _vgfx, p: DrawLabelParams| {
            let DrawLabelParams {
                label_id,
                x,
                y,
                max_width,
            } = p;
            if let Some(label) = _vgfx.labels.get(&label_id) {
                let canvas = &mut _vgfx
                    .canvas
                    .try_lock()
                    .map_err(|_| mlua::Error::external("Canvas in use".to_string()))?;
                let paint = _vgfx
                    .fill_paint
                    .clone()
                    .unwrap_or_else(|| _vgfx.stroke_paint.clone())
                    .with_font_size(label.size as f32);

                let text_measure = canvas
                    .measure_text(x, y, &label.text, &paint)
                    .map_err(mlua::Error::external)?;

                let x_scale = (max_width / text_measure.width()).min(1.0);

                let paint = paint.with_font_size(label.size as f32 * x_scale);

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
        methods.add_method_mut("MoveTo", |_, _vgfx, p: MoveToParams| {
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
        methods.add_method_mut("LineTo", |_, _vgfx, p: LineToParams| {
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
        methods.add_method_mut("BezierTo", |_, _vgfx, p: BezierToParams| {
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
        });

        //QuadTo
        tealr::mlu::create_named_parameters!(QuadToParams with
          cx : f32,
          cy : f32,
          x : f32,
          y : f32,

        );
        methods.add_method_mut("QuadTo", |_, _vgfx, p: QuadToParams| {
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
        methods.add_method_mut("ArcTo", |_, _vgfx, p: ArcToParams| {
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
        methods.add_method_mut("ClosePath", |_, _vgfx, _: ()| {
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
        methods.add_method_mut("MiterLimit", |_, _vgfx, p: MiterLimitParams| {
            _vgfx.stroke_paint.set_miter_limit(p.limit);
            Ok(())
        });

        //StrokeWidth
        tealr::mlu::create_named_parameters!(StrokeWidthParams with
          size : f32,

        );
        methods.add_method_mut("StrokeWidth", |_, _vgfx, p: StrokeWidthParams| {
            _vgfx.stroke_paint.set_line_width(p.size);
            Ok(())
        });

        //LineCap
        tealr::mlu::create_named_parameters!(LineCapParams with
          cap : u8,

        );
        methods.add_method_mut("LineCap", |_, _vgfx, p: LineCapParams| {
            _vgfx
                .stroke_paint
                .set_line_cap(unsafe { std::mem::transmute(p.cap) });
            Ok(())
        });

        //LineJoin
        tealr::mlu::create_named_parameters!(LineJoinParams with
          join : u8,

        );
        methods.add_method_mut("LineJoin", |_, _vgfx, p: LineJoinParams| {
            _vgfx
                .stroke_paint
                .set_line_join(unsafe { std::mem::transmute(p.join) });
            Ok(())
        });

        //Stroke
        methods.add_method_mut("Stroke", |_, _vgfx, _: ()| {
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
        methods.add_method_mut("StrokeColor", |_, _vgfx, p: StrokeColorParams| {
            let StrokeColorParams { r, g, b, a } = p;
            _vgfx
                .stroke_paint
                .set_color(Color::rgba(r, g, b, a.unwrap_or(255))); //TODO
            Ok(())
        });

        //UpdateLabel
        tealr::mlu::create_named_parameters!(UpdateLabelParams with
          label_id : u32,
          text : String,
          size : i32,

        );
        methods.add_method_mut("UpdateLabel", |_, _vgfx, p: UpdateLabelParams| {
            if let Some(label) = _vgfx.labels.get_mut(&p.label_id) {
                label.text = p.text;
                label.size = p.size;
                Ok(())
            } else {
                Err(mlua::Error::external(format!(
                    "No label with id {}",
                    p.label_id
                )))
            }
        });

        //DrawGauge
        tealr::mlu::create_named_parameters!(DrawGaugeParams with
          rate : f32,
          x : f32,
          y : f32,
          w : f32,
          h : f32,
          delta_time : f32,

        );
        methods.add_method_mut(
            "DrawGauge",
            |_, _vgfx, _: DrawGaugeParams| -> Result<(), mlua::Error> {
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
        methods.add_method_mut(
            "SetGaugeColor",
            |_, _vgfx, _: SetGaugeColorParams| -> Result<(), mlua::Error> {
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
        methods.add_method_mut("RoundedRect", |_, _vgfx, p: RoundedRectParams| {
            let RoundedRectParams { x, y, w, h, r } = p;
            if let Some(path) = _vgfx.path.as_mut() {
                path.rounded_rect(x, y, w, h, r);
                Ok(())
            } else {
                Err(mlua::Error::external("No path started".to_string()))
            }
        });

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
        methods.add_method_mut(
            "RoundedRectVarying",
            |_, _vgfx, p: RoundedRectVaryingParams| {
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
        methods.add_method_mut("Ellipse", |_, _vgfx, p: EllipseParams| {
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
        methods.add_method_mut("Circle", |_, _vgfx, p: CircleParams| {
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
        methods.add_method_mut("SkewX", |_, _vgfx, p: SkewXParams| {
            _vgfx.with_canvas(|canvas| canvas.skew_x(p.angle))?;
            Ok(())
        });

        //SkewY
        tealr::mlu::create_named_parameters!(SkewYParams with
          angle : f32,

        );
        methods.add_method_mut("SkewY", |_, _vgfx, p: SkewYParams| {
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
        methods.add_method_mut("LinearGradient", |_, _vgfx, p: LinearGradientParams| {
            let LinearGradientParams { sx, sy, ex, ey } = p;
            _vgfx.paints.insert(
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
        });

        //BoxGradient
        tealr::mlu::create_named_parameters!(BoxGradientParams with
          x : f32,
          y : f32,
          w : f32,
          h : f32,
          r : f32,
          f : f32,

        );
        methods.add_method_mut("BoxGradient", |_, _vgfx, p: BoxGradientParams| {
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
        });

        //RadialGradient
        tealr::mlu::create_named_parameters!(RadialGradientParams with
          cx : f32,
          cy : f32,
          inr : f32,
          outr : f32,

        );
        methods.add_method_mut("RadialGradient", |_, _vgfx, p: RadialGradientParams| {
            let RadialGradientParams { cx, cy, inr, outr } = p;
            _vgfx.paints.insert(
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
        });

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
        methods.add_method_mut("ImagePattern", |_, _vgfx, p: ImagePatternParams| {
            let ImagePatternParams {
                ox,
                oy,
                ex,
                ey,
                angle,
                image,
                alpha,
            } = p;

            if let Some(id) = _vgfx.images.get(&image) {
                let paint = Paint::image(*id, ox, oy, ex, ey, angle, alpha);
                _vgfx.paints.insert(_vgfx.next_paint_id, paint);
                let paint_id = _vgfx.next_paint_id;
                _vgfx.next_paint_id += 1;
                _vgfx.paint_imgs.insert(paint_id, *id);
                Ok(paint_id)
            } else {
                Err(mlua::Error::external(format!("No image with id {image}")))
            }
        });

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
        methods.add_method_mut(
            "UpdateImagePattern",
            |_, _vgfx, p: UpdateImagePatternParams| {
                let UpdateImagePatternParams {
                    paint,
                    ox,
                    oy,
                    ex,
                    ey,
                    angle,
                    alpha,
                } = p;

                if let (Some(pattern_paint), Some(img)) =
                    (_vgfx.paints.get_mut(&paint), _vgfx.paint_imgs.get(&paint))
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
        methods.add_method_mut("GradientColors", |_, _vgfx, p: GradientColorsParams| {
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
        });

        //FillPaint
        tealr::mlu::create_named_parameters!(FillPaintParams with
          paint : u32,

        );
        methods.add_method_mut("FillPaint", |_, _vgfx, p: FillPaintParams| {
            if let Some(paint) = _vgfx.paints.get(&p.paint) {
                _vgfx.fill_paint = Some(paint.clone());
            }
            Ok(())
        });

        //StrokePaint
        tealr::mlu::create_named_parameters!(StrokePaintParams with
          paint : u32,

        );
        methods.add_method_mut("StrokePaint", |_, _vgfx, p: StrokePaintParams| {
            if let Some(paint) = _vgfx.paints.get(&p.paint) {
                _vgfx.stroke_paint = paint.clone();
            }

            Ok(())
        });

        //Save
        methods.add_method_mut("Save", |_l, _vgfx, _: ()| {
            _vgfx.with_canvas(|canvas| canvas.save())?;
            //TODO: stacks for custom stuff
            Ok(())
        });

        //Restore
        methods.add_method_mut("Restore", |_, _vgfx, _: ()| {
            _vgfx.with_canvas(|canvas| canvas.restore())?;
            //TODO: stacks for custom stuff
            Ok(())
        });

        //Reset
        methods.add_method_mut("Reset", |_, _vgfx, _: ()| {
            _vgfx.with_canvas(|canvas| canvas.reset())
        });

        //PathWinding
        tealr::mlu::create_named_parameters!(PathWindingParams with
          dir : i32,

        );
        methods.add_method_mut("PathWinding", |_, _vgfx, p: PathWindingParams| {
            todo!();
            Ok(0)
        });

        //ForceRender
        methods.add_method_mut("ForceRender", |_, _vgfx, _: ()| {
            //TODO: Flush game render as well
            _vgfx.with_canvas(|canvas| canvas.flush())?;
            Ok(())
        });

        //LoadImageJob
        tealr::mlu::create_named_parameters!(LoadImageJobParams with
          path : String,
          placeholder : i32,
          w : i32,
          h : i32,

        );
        methods.add_method_mut("LoadImageJob", |_, _vgfx, p: LoadImageJobParams| Ok(0));

        //LoadWebImageJob
        tealr::mlu::create_named_parameters!(LoadWebImageJobParams with
          url : String,
          placeholder : i32,
          w : i32,
          h : i32,

        );
        methods.add_method_mut("LoadWebImageJob", |_, _vgfx, p: LoadWebImageJobParams| {
            todo!();
            Ok(0)
        });

        //Scissor
        tealr::mlu::create_named_parameters!(ScissorParams with
          x : f32,
          y : f32,
          w : f32,
          h : f32,

        );
        methods.add_method_mut("Scissor", |_, _vgfx, p: ScissorParams| {
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
        methods.add_method_mut("IntersectScissor", |_, _vgfx, p: IntersectScissorParams| {
            let IntersectScissorParams { x, y, w, h } = p;
            _vgfx.with_canvas(|canvas| canvas.intersect_scissor(x, y, w, h))?;
            Ok(())
        });

        //ResetScissor
        methods.add_method_mut("ResetScissor", |_, _vgfx, _: ()| {
            _vgfx.with_canvas(|canvas| canvas.reset_scissor())?;
            Ok(())
        });

        //TextBounds
        tealr::mlu::create_named_parameters!(TextBoundsParams with
          x : f32,
          y : f32,
          s : String,

        );
        methods.add_method_mut("TextBounds", |_, _vgfx, p: TextBoundsParams| {
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
        });

        //LabelSize
        tealr::mlu::create_named_parameters!(LabelSizeParams with
          label : i32,

        );
        methods.add_method_mut("LabelSize", |_, _vgfx, p: LabelSizeParams| {
            todo!();
            Ok(0)
        });

        //FastTextSize
        tealr::mlu::create_named_parameters!(FastTextSizeParams with
          text : String,

        );
        methods.add_method_mut("FastTextSize", |_, _vgfx, p: FastTextSizeParams| {
            todo!();
            Ok(0)
        });

        //ImageSize
        tealr::mlu::create_named_parameters!(ImageSizeParams with
          image : u32,

        );
        methods.add_method_mut("ImageSize", |_, _vgfx, p: ImageSizeParams| {
            if let Some(id) = _vgfx.images.get(&p.image).copied() {
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
        methods.add_method_mut("Arc", |_, _vgfx, p: ArcParams| {
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
          r : i32,
          g : i32,
          b : i32,

        );
        methods.add_method_mut("SetImageTint", |_, _vgfx, p: SetImageTintParams| {
            if let Some(paint) = _vgfx.fill_paint.as_mut() {
                //Paint::image_tint(id, cx, cy, width, height, angle, tint)
            }
            todo!();
            Ok(0)
        });

        //GlobalCompositeOperation
        tealr::mlu::create_named_parameters!(GlobalCompositeOperationParams with
          op : u8,

        );
        methods.add_method_mut(
            "GlobalCompositeOperation",
            |_, _vgfx, p: GlobalCompositeOperationParams| {
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
        methods.add_method_mut(
            "GlobalCompositeBlendFunc",
            |_, _vgfx, p: GlobalCompositeBlendFuncParams| {
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
        methods.add_method_mut(
            "GlobalCompositeBlendFuncSeparate",
            |_, _vgfx, p: GlobalCompositeBlendFuncSeparateParams| {
                todo!();
                Ok(0)
            },
        );

        //LoadAnimation
        tealr::mlu::create_named_parameters!(LoadAnimationParams with
          path : String,
          frametime : f32,
          loopcount : i32,
          compressed : bool,

        );
        methods.add_method_mut("LoadAnimation", |_, _vgfx, p: LoadAnimationParams| {
            todo!();
            Ok(0)
        });

        //GlobalAlpha
        tealr::mlu::create_named_parameters!(GlobalAlphaParams with
          alpha : f32,

        );
        methods.add_method_mut("GlobalAlpha", |_, _vgfx, p: GlobalAlphaParams| {
            todo!();
            Ok(0)
        });

        //LoadSkinAnimation
        tealr::mlu::create_named_parameters!(LoadSkinAnimationParams with
          path : String,
          frametime : f32,
          loopcount : i32,
          compressed : bool,

        );
        methods.add_method_mut(
            "LoadSkinAnimation",
            |_, _vgfx, p: LoadSkinAnimationParams| {
                todo!();
                Ok(0)
            },
        );

        //TickAnimation
        tealr::mlu::create_named_parameters!(TickAnimationParams with
          animation : i32,
          delta_time : f32,

        );
        methods.add_method_mut("TickAnimation", |_, _vgfx, p: TickAnimationParams| {
            todo!();
            Ok(0)
        });

        //LoadSharedTexture
        tealr::mlu::create_named_parameters!(LoadSharedTextureParams with
          key : String,
          path : String,

        );
        methods.add_method_mut(
            "LoadSharedTexture",
            |_, _vgfx, p: LoadSharedTextureParams| {
                todo!();
                Ok(0)
            },
        );

        //LoadSharedSkinTexture
        tealr::mlu::create_named_parameters!(LoadSharedSkinTextureParams with
          key : String,
          path : String,

        );
        methods.add_method_mut(
            "LoadSharedSkinTexture",
            |_, _vgfx, p: LoadSharedSkinTextureParams| {
                todo!();
                Ok(0)
            },
        );

        //GetSharedTexture
        tealr::mlu::create_named_parameters!(GetSharedTextureParams with
          key : String,

        );
        methods.add_method_mut("GetSharedTexture", |_, _vgfx, p: GetSharedTextureParams| {
            todo!();
            Ok(0)
        });
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
        instance_collector.add_instance("Vgfx", UserDataProxy::<Vgfx>::new)?;
        Ok(())
    }
}

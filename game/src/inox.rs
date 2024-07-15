use crate::config::GameConfig;
use crate::help::{LuaHelpers, ToLuaResult};
use crate::vg_ui::Vgfx;
use femtovg::{ImageFlags, ImageInfo};
use futures::TryFutureExt;
use inox2d::formats::inp::parse_inp;
use inox2d::model::Model;
use inox2d_opengl::OpenglRenderer;
use log::info;
use std::borrow::Borrow;
use std::cell::Cell;
use std::path::PathBuf;
use std::sync::{RwLock, Weak};
use std::{collections::HashMap, sync::Arc};
use tealr::{
    mlu::{mlua::ExternalError, TealData, UserData},
    mlua_create_named_parameters, ToTypename,
};
use three_d::{ClearState, DepthTarget, DepthTexture2D, DepthTextureDataType, Matrix, Viewport};

use inox2d::render::{InoxRenderer, InoxRendererCommon};

#[derive(UserData, ToTypename, Clone)]
pub struct Inox {
    renderer: Arc<RwLock<OpenglRenderer>>,
    gl: three_d::Context,
}

impl Inox {
    pub fn new(renderer: Arc<RwLock<OpenglRenderer>>, gl: three_d::Context) -> Self {
        Self { renderer, gl }
    }
}

impl TealData for Inox {
    fn add_methods<'lua, T: tealr::mlu::TealDataMethods<'lua, Self>>(methods: &mut T) {
        mlua_create_named_parameters!(LoadModelParams with
            path: String,
            w: u32,
            h: u32,
        );

        methods.add_method("LoadModel", |lua, inox, LoadModelParams { path, w, h }| {
            let mut model_path = GameConfig::get().skin_path();
            model_path.push("models2d");
            model_path.push(path);
            let data = std::fs::read(model_path).to_lua()?;
            let model = inox2d::formats::inp::parse_inp(data.as_slice()).to_lua()?;
            let opengl_renderer = &mut inox.renderer.write().unwrap();
            opengl_renderer.resize(w, h);
            opengl_renderer.prepare(&model).to_lua()?;

            let res = lua.game_data()?.read().unwrap().resolution;

            inox.gl.set_viewport(Viewport {
                x: 0,
                y: 0,
                width: res.0,
                height: res.1,
            });

            Ok(InoxModel::new(
                model,
                Arc::downgrade(&inox.renderer),
                inox.gl.clone(),
                w,
                h,
                lua.vgfx()?,
            ))
        });
    }

    fn add_fields<'lua, F: tealr::mlu::TealDataFields<'lua, Self>>(_fields: &mut F) {}
}
#[derive(ToTypename, UserData)]
pub struct InoxModel {
    model: Model,
    renderer: Weak<RwLock<OpenglRenderer>>,
    params: Vec<(String, f32, f32)>,
    gl: three_d::Context,
    texture: three_d::Texture2D,
    image: femtovg::ImageId,
    depth: DepthTexture2D,
}

struct f24d8;
impl three_d::texture::DepthDataType for f24d8 {
    fn internal_format() -> u32 {
        three_d::context::DEPTH24_STENCIL8
    }
}
impl DepthTextureDataType for f24d8 {}

impl InoxModel {
    pub fn new(
        model: Model,
        renderer: Weak<RwLock<OpenglRenderer>>,
        gl: three_d::Context,
        width: u32,
        height: u32,
        vgfx: Arc<RwLock<Vgfx>>,
    ) -> Self {
        let texture = three_d::Texture2D::new_empty::<[u8; 4]>(
            &gl,
            width,
            height,
            three_d::Interpolation::Linear,
            three_d::Interpolation::Linear,
            None,
            three_d::Wrapping::ClampToEdge,
            three_d::Wrapping::ClampToEdge,
        );

        let depth = DepthTexture2D::new::<f24d8>(
            &gl,
            width,
            height,
            three_d::Wrapping::ClampToEdge,
            three_d::Wrapping::ClampToEdge,
        );

        let vgfx = vgfx.write().unwrap();
        let canvas = &mut vgfx.canvas.lock().unwrap();

        Self {
            model,
            renderer,
            gl,
            params: vec![],
            image: canvas
                .create_image_from_native_texture(
                    texture.raw_id(),
                    ImageInfo::new(
                        ImageFlags::FLIP_Y,
                        width as _,
                        height as _,
                        femtovg::PixelFormat::Rgba8,
                    ),
                )
                .expect("Could not create image"),
            texture,
            depth,
        }
    }
}

impl TealData for InoxModel {
    fn add_methods<'lua, T: tealr::mlu::TealDataMethods<'lua, Self>>(methods: &mut T) {
        mlua_create_named_parameters!(InoxRenderParams with x: f32, y: f32, w: u32, h: u32, dt: f32,);

        methods.add_method_mut(
            "Render",
            |lua, data, InoxRenderParams { x, y, w, h, dt }| {
                let handle = data.renderer.upgrade().expect("Renderer dropped");
                let mut renderer = handle.write().unwrap();
                renderer.clear();
                let vgfx_handle = lua.vgfx()?;
                let vgfx = vgfx_handle.write().unwrap();
                renderer.camera.scale.x = 0.33;
                renderer.camera.scale.y = 0.33;
                let render_width = data.texture.width();
                let render_height = data.texture.height();

                {
                    let target = three_d::RenderTarget::new(
                        data.texture.as_color_target(None),
                        data.depth.as_depth_stencil_target(),
                    );

                    _ = target
                        .clear(ClearState::color_and_depth(0.0, 0.0, 0.0, 0.0, 1.0))
                        .write(|| {
                            data.gl.set_viewport(Viewport {
                                x: 0,
                                y: 0,
                                width: render_width,
                                height: render_height,
                            });

                            data.model.puppet.begin_set_params();
                            for (name, x, y) in data.params.drain(..) {
                                data.model
                                    .puppet
                                    .set_named_param(&name, (x, y).into())
                                    .to_lua()?;
                            }

                            data.model.puppet.end_set_params(dt);
                            renderer.render(&data.model.puppet);
                            data.gl.error_check().to_lua()
                        });
                }
                let res = lua.game_data()?.read().unwrap().resolution;

                data.gl.set_viewport(Viewport {
                    x: 0,
                    y: 0,
                    width: res.0,
                    height: res.1,
                });
                let mut canvas = vgfx.canvas.lock().unwrap();
                let img_id = data.image;
                canvas.save_with(|canvas| {
                    let (img_w, img_h) = canvas
                        .image_size(img_id)
                        .map_err(tealr::mlu::mlua::Error::external)
                        .unwrap_or((1, 1));
                    let scale_x = w as f32 / img_w as f32;
                    let scale_y = h as f32 / img_h as f32;
                    canvas.translate(x, y);

                    canvas.scale(scale_x, scale_y);
                    let paint = femtovg::Paint::image_tint(
                        img_id,
                        0.0,
                        0.0,
                        img_w as f32,
                        img_h as f32,
                        0.0,
                        femtovg::Color {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 1.0,
                        },
                    );
                    let mut rect = femtovg::Path::new();
                    rect.rect(0.0, 0.0, img_w as f32, img_h as f32);
                    canvas.fill_path(&rect, &paint);
                });

                Ok(())
            },
        );

        mlua_create_named_parameters!(InoxPuppetParam with name: String, x: f32, y: f32,);

        methods.add_method_mut("SetParam", |_lua, data, InoxPuppetParam { name, x, y }| {
            data.params.push((name, x, y));
            Ok(())
        })
    }
}

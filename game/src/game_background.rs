use std::{
    cell::RefMut,
    path::{Path, PathBuf},
    rc::{Rc, Weak},
    sync::{Arc, Mutex},
};

use crate::{
    game_data::{ExportGame, GameData, LuaPath},
    shaded_mesh::ShadedMesh,
    vg_ui::{ExportVgfx, Vgfx},
};
use generational_arena::Index;
use glow::HasContext;
use kson::MeasureBeatLines;
use log::warn;
use puffin::profile_function;
use tealr::{
    mlu::{
        mlua::{self},
        UserData,
    },
    mlu::{
        mlua::{Function, Lua, LuaOptions},
        TealData, UserDataProxy,
    },
    mlua_create_named_parameters, TypeName,
};
use three_d_asset::{vec2, vec3, Vec2, Vector2, Vector3, Viewport};

#[derive(Debug, Clone, Copy)]
pub struct BackgroundData {
    screen_center: (f32, f32),
    /// (beat, offsync, playback)
    timing: (f32, f32, f32),
    roll: f32,
    clear_transition: f32,
    speed_mult: f32,
    viewport: Viewport,
}

impl Default for BackgroundData {
    fn default() -> Self {
        Self {
            screen_center: Default::default(),
            timing: Default::default(),
            roll: Default::default(),
            clear_transition: Default::default(),
            speed_mult: 1.0,
            viewport: Viewport {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        }
    }
}

pub struct GameBackground {
    name: String,
    lua: Lua,
    beat_bounds: (f64, f64),
    beat_iter: MeasureBeatLines,
    vgfx: Arc<Mutex<Vgfx>>,
    background: bool,
}

#[derive(UserData, TypeName)]
struct GameBackgroundLua;

impl TealData for GameBackgroundLua {
    fn add_methods<'lua, T: tealr::mlu::TealDataMethods<'lua, Self>>(methods: &mut T) {
        mlua_create_named_parameters!(LoadTextureParams with
            shadername : String,
            filename : String,
        );
        methods.add_function(
            "LoadTexture",
            |lua,
             LoadTextureParams {
                 shadername,
                 filename,
             }| {
                let mut path = { lua.app_data_ref::<PathBuf>().expect("No path set").clone() };

                let bg = &mut lua
                    .app_data_mut::<ShadedMesh>()
                    .expect("Background or Foreground mesh data not set");

                path.push(filename);

                bg.use_texture(shadername, path, (true, true))
                    .map_err(mlua::Error::external)?;

                Ok(())
            },
        );

        mlua_create_named_parameters!(SetParamiParams with name : String, value : i32,);
        methods.add_function("SetParami", |lua, SetParamiParams { name, value }| {
            let bg = &mut lua
                .app_data_mut::<ShadedMesh>()
                .expect("Background or Foreground mesh data not set");
            bg.set_param(name, value);
            Ok(())
        });

        mlua_create_named_parameters!(SetParamfParams with name : String, value : f32,);
        methods.add_function("SetParamf", |lua, SetParamfParams { name, value }| {
            let bg = &mut lua
                .app_data_mut::<ShadedMesh>()
                .expect("Background or Foreground mesh data not set");
            bg.set_param(name, value);
            Ok(())
        });

        methods.add_function("GetPath", |lua, _: ()| {
            Ok(lua
                .app_data_ref::<PathBuf>()
                .and_then(|x| x.to_str().map(|x| x.to_string() + "/")))
        });

        methods.add_function("GetTiming", |lua, _: ()| {
            Ok(lua
                .app_data_ref::<BackgroundData>()
                .map(|x| x.timing)
                .unwrap_or_default())
        });

        methods.add_function("GetTilt", |lua, _: ()| {
            Ok(lua
                .app_data_ref::<BackgroundData>()
                .map(|x| x.roll)
                .unwrap_or_default())
        });

        methods.add_function("GetScreenCenter", |lua, _: ()| {
            Ok(lua
                .app_data_ref::<BackgroundData>()
                .map(|x| x.screen_center)
                .unwrap_or_default())
        });

        methods.add_function("GetClearTransition", |lua, _: ()| {
            Ok(lua
                .app_data_ref::<BackgroundData>()
                .map(|x| x.clear_transition)
                .unwrap_or_default())
        });

        methods.add_function("SetSpeedMult", |lua, speed: f32| {
            if let Some(mut data) = lua.app_data_mut::<BackgroundData>() {
                data.speed_mult = speed;
            }
            Ok(())
        });

        methods.add_function("DrawShader", |lua, _: ()| {
            /* Shader uniforms:
            uniform ivec2 screenCenter;
            // x = bar time
            // y = off-sync but smooth bpm based timing
            // z = real time since song start
            uniform vec3 timing;
            uniform ivec2 viewport;
            uniform float objectGlow;
            // bg_texture.png
            uniform sampler2D mainTex;
            uniform sampler2D backTex;
            uniform vec2 tilt;
            uniform float clearTransition;
                         */

            let data = {
                lua.app_data_ref::<BackgroundData>()
                    .map(|x| *x)
                    .expect("Background data not set")
            };
            let bg = &mut lua
                .app_data_mut::<ShadedMesh>()
                .expect("Background mesh not set");

            bg.set_param(
                "screenCenter",
                Vector2::new((data.screen_center.0) as i32, (data.screen_center.1) as i32),
            );

            bg.set_param("timing", Vector3::from(data.timing));
            bg.set_param("clearTransition", data.clear_transition);
            bg.set_param("tilt", vec2(data.roll, 0.0)); //(camera roll, background spin)

            bg.draw_fullscreen(data.viewport);
            Ok(())
        })
    }

    fn add_fields<'lua, F: tealr::mlu::TealDataFields<'lua, Self>>(_fields: &mut F) {}
}

impl Drop for GameBackground {
    fn drop(&mut self) {
        if let Ok(mut vgfx) = self.vgfx.lock() {
            vgfx.drop_assets(&Self::gen_index(self.background));
        }
    }
}

impl GameBackground {
    fn gen_index(background: bool) -> Index {
        Index::from_raw_parts(5000, if background { 10 } else { 20 }) //TODO: Shouldn't just assume this won't collide with anything
    }

    pub fn new(
        context: &three_d::Context,
        background: bool,
        path: impl AsRef<Path>,
        chart: &kson::Chart,
        vgfx: Arc<Mutex<Vgfx>>,
        game_data: Arc<Mutex<GameData>>,
    ) -> anyhow::Result<Self> {
        use mlua::StdLib;
        let mut path = path.as_ref().to_path_buf();

        let name = if background { "bg" } else { "fg" };
        let full_name = if background {
            "background"
        } else {
            "foreground"
        };
        {
            vgfx.lock()
                .unwrap()
                .init_asset_scope(Self::gen_index(background))
        }

        path.push(name);
        path.set_extension("fs");
        let fs = std::io::read_to_string(std::fs::File::open(&path)?)?;
        let mesh = ShadedMesh::new_fullscreen(context, &fs)?;

        let lua = Lua::new_with(StdLib::MATH | StdLib::STRING, LuaOptions::new())?;
        lua.globals().set(full_name, GameBackgroundLua)?;

        tealr::mlu::set_global_env(ExportVgfx, &lua)?;
        tealr::mlu::set_global_env(ExportGame, &lua)?;
        tealr::mlu::set_global_env(LuaPath, &lua)?;

        lua.set_app_data(vgfx.clone());
        lua.set_app_data(game_data.clone());
        lua.set_app_data(mesh);
        lua.set_app_data(BackgroundData::default());
        lua.set_app_data(Self::gen_index(background));

        let mut beat_iter = chart.beat_line_iter();

        let game_background = Self {
            name: format!("render_{name}"),
            lua,
            beat_bounds: (0.0, chart.tick_to_ms(beat_iter.next().unwrap().0)),
            beat_iter,
            vgfx,
            background,
        };
        path.set_extension("lua");

        let lua = std::io::read_to_string(std::fs::File::open(&path)?)?;
        path.pop();
        game_background.lua.set_app_data(path);
        game_background.lua.load(&lua).exec()?;
        Ok(game_background)
    }

    pub fn render(
        &mut self,
        dt: f64,
        camera: &three_d::Camera,
        chart_time: f64,
        chart: &kson::Chart,
        tick: u32,
        roll: f32,
        clear: bool,
    ) {
        profile_function!();
        let center = camera.pixel_at_position(vec3(0.0, -50.0, 0.0));
        let bpm = chart.bpm_at_tick(tick);

        {
            let data = &mut self
                .lua
                .app_data_mut::<BackgroundData>()
                .expect("No background data");
            data.screen_center = center;
            data.viewport = camera.viewport();

            while chart_time > self.beat_bounds.1 {
                self.beat_bounds.0 = self.beat_bounds.1;
                self.beat_bounds.1 = chart.tick_to_ms(self.beat_iter.next().unwrap().0);
            }

            data.timing.0 = ((chart_time - self.beat_bounds.0)
                / (self.beat_bounds.1 - self.beat_bounds.0))
                .clamp(0.0, 1.0) as f32;
            data.timing.1 += data.speed_mult * (dt / kson::beat_in_ms(bpm)) as f32;
            data.timing.2 = chart_time as f32 / 1000.0;

            data.roll = roll / 360.0;

            data.clear_transition = (data.clear_transition
                + if clear {
                    dt / kson::beat_in_ms(bpm)
                } else {
                    -dt / kson::beat_in_ms(bpm)
                } as f32)
                .clamp(0.0, 1.0);
        }

        if let Ok(render_fn) = self.lua.globals().get::<_, Function>(self.name.as_str()) {
            if let Some(e) = render_fn.call::<_, ()>(dt / 1000.0).err() {
                warn!("{} error: {:?}", &self.name, e);
            }
        } else {
            warn!("No render fn");
        }
    }
}

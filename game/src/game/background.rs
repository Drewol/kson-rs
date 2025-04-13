use std::path::{Path, PathBuf};

use crate::{
    game::ChartView,
    game_data::{GameData, GameDataLua, LuaPath},
    help::transform_shader,
    lua_service::set_global_env,
    shaded_mesh::ShadedMesh,
    util::lua_address,
    vg_ui::{Vgfx, VgfxLua},
};

use di::RefMut;
use kson::MeasureBeatLines;
use log::warn;
use mlua::{Function, Lua, LuaOptions, LuaSerdeExt, UserData, UserDataMethods};
use puffin::profile_function;

use serde::Serialize;
use three_d_asset::{vec2, Vector2, Vector3, Viewport};

#[derive(Debug, Clone, Copy)]
pub struct BackgroundData {
    screen_center: (f32, f32),
    /// (beat, offsync, playback)
    timing: (f32, f32, f32),
    // (laser, spin)
    roll: (f32, f32),
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
    vgfx: RefMut<Vgfx>,
    background: bool,
}

struct GameBackgroundLua;

impl UserData for GameBackgroundLua {
    fn add_methods<T: UserDataMethods<Self>>(methods: &mut T) {
        methods.add_function(
            "LoadTexture",
            |lua, (shadername, filename): (String, String)| {
                let mut path = { lua.app_data_ref::<PathBuf>().expect("No path set").clone() };

                let bg = &mut lua
                    .app_data_mut::<ShadedMesh>()
                    .expect("Background or Foreground mesh data not set");

                path.push(filename);

                bg.use_texture(shadername, path, (true, true), true)
                    .map_err(mlua::Error::external)?;

                Ok(())
            },
        );

        methods.add_function("SetParami", |lua, (name, value): (String, i32)| {
            let bg = &mut lua
                .app_data_mut::<ShadedMesh>()
                .expect("Background or Foreground mesh data not set");
            bg.set_param(name.as_str(), value);
            Ok(())
        });

        methods.add_function("SetParamf", |lua, (name, value): (String, f32)| {
            let bg = &mut lua
                .app_data_mut::<ShadedMesh>()
                .expect("Background or Foreground mesh data not set");
            bg.set_param(name.as_str(), value);
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
            bg.set_param("tilt", vec2(data.roll.0, data.roll.1)); //(camera roll, background spin)
            if let Some(e) = bg
                .set_tex_from_framebuffer(data.viewport.width as _, data.viewport.height as _)
                .err()
            {
                warn!("Failed to set framebuffer texture: {e}");
            };

            bg.draw_fullscreen(data.viewport);
            Ok(())
        })
    }
}

impl Drop for GameBackground {
    fn drop(&mut self) {
        if let Ok(mut vgfx) = self.vgfx.write() {
            vgfx.drop_assets(lua_address(&self.lua));
        }
    }
}

impl GameBackground {
    pub fn new(
        context: &three_d::Context,
        background: bool,
        path: impl AsRef<Path>,
        chart: &kson::Chart,
        vgfx: RefMut<Vgfx>,
        game_data: RefMut<GameData>,
    ) -> anyhow::Result<Self> {
        use mlua::StdLib;
        let mut path = path.as_ref().to_path_buf();

        let name = if background { "bg" } else { "fg" };
        let full_name = if background {
            "background"
        } else {
            "foreground"
        };

        path.push(name);
        path.set_extension("fs");
        let fs = transform_shader(std::io::read_to_string(std::fs::File::open(&path)?)?);

        let mesh = ShadedMesh::new_fullscreen(context, &fs)?;

        let lua = Lua::new_with(StdLib::MATH | StdLib::STRING, LuaOptions::new())?;
        lua.globals().set(full_name, GameBackgroundLua)?;

        {
            vgfx.write()
                .expect("Lock error")
                .init_asset_scope(lua_address(&lua))
        }

        set_global_env(VgfxLua, "gfx", &lua)?;
        set_global_env(GameDataLua, "game", &lua)?;
        set_global_env(LuaPath, "path", &lua)?;

        lua.set_app_data(vgfx.clone());
        lua.set_app_data(game_data.clone());
        lua.set_app_data(mesh);
        lua.set_app_data(BackgroundData::default());

        let mut beat_iter = chart.beat_line_iter();

        let game_background = Self {
            name: format!("render_{name}"),
            lua,
            beat_bounds: (
                0.0,
                chart.tick_to_ms(beat_iter.next().map(|x| x.0).unwrap_or(u32::MAX)),
            ),
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

    pub fn set_global(&mut self, name: &str, value: &impl Serialize) {
        profile_function!();
        let Ok(value) = self.lua.to_value(value) else {
            warn!("Failed to convert input to lua");
            return;
        };

        _ = self.lua.globals().set(name, value);
    }

    pub fn render(
        &mut self,
        dt: f64,
        camera: &three_d::Camera,
        chart_time: f64,
        chart: &kson::Chart,
        tick: u32,
        roll: (f32, f32),
        clear: bool,
    ) {
        profile_function!();
        let center = camera.pixel_at_position(ChartView::TRACK_DIRECTION * -300.0);
        let bpm = chart.bpm_at_tick(tick);

        {
            let data = &mut self
                .lua
                .app_data_mut::<BackgroundData>()
                .expect("No background data");
            data.screen_center = center.into();
            data.viewport = camera.viewport();

            while chart_time > self.beat_bounds.1 {
                self.beat_bounds.0 = self.beat_bounds.1;
                self.beat_bounds.1 =
                    chart.tick_to_ms(self.beat_iter.next().map(|x| x.0).unwrap_or(u32::MAX));
            }

            data.timing.0 = ((chart_time - self.beat_bounds.0)
                / (self.beat_bounds.1 - self.beat_bounds.0))
                .clamp(0.0, 1.0) as f32;
            data.timing.1 += data.speed_mult * (dt / kson::beat_in_ms(bpm)) as f32;
            data.timing.2 = chart_time as f32 / 1000.0;

            data.roll = (roll.0 / -360.0, roll.1 / -360.0);

            data.clear_transition = (data.clear_transition
                + if clear {
                    dt / kson::beat_in_ms(bpm)
                } else {
                    -dt / kson::beat_in_ms(bpm)
                } as f32)
                .clamp(0.0, 1.0);
        }

        if let Ok(render_fn) = self.lua.globals().get::<Function>(self.name.as_str()) {
            if let Some(e) = render_fn.call::<()>(dt / 1000.0).err() {
                warn!("{} error: {}", &self.name, e);
            }
        } else {
            warn!("No render fn");
        }
    }
}

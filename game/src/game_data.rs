use std::{
    collections::HashMap,
    ops::DerefMut,
    path::PathBuf,
    sync::{atomic::AtomicUsize, Arc},
    time::Duration,
};

use di::{Activator, InjectBuilder, Injectable, RefMut};
use egui::epaint::Hsva;
use log::warn;

use mlua::{AppDataRef, Lua, UserData, UserDataFields, UserDataMethods};
use puffin::{ProfilerScope, ThreadProfiler};
use rodio::Source;

use crate::{
    button_codes::UscButton, config::GameConfig, input_state::InputState, lua_service::LuaKey,
    skin_settings::SkinSettingValue, RuscMixer,
};

pub struct GameData {
    pub resolution: (u32, u32),
    pub mouse_pos: (f64, f64),
    pub profile_stack: Vec<ProfilerScope>,
    pub input_state: InputState,
    pub audio_samples: HashMap<String, rodio::source::Buffered<rodio::Decoder<std::fs::File>>>,
    pub audio_sample_play_status: HashMap<String, Arc<AtomicUsize>>,
}

impl Injectable for GameData {
    fn inject(lifetime: di::ServiceLifetime) -> di::InjectBuilder {
        InjectBuilder::new(
            Activator::new::<Self, Self>(
                |sp| {
                    Arc::new(GameData {
                        resolution: (800, 600),
                        mouse_pos: (0.0, 0.0),
                        profile_stack: vec![],
                        input_state: InputState::clone(&sp.get_required()),
                        audio_samples: Default::default(),
                        audio_sample_play_status: Default::default(),
                    })
                },
                |sp| {
                    Arc::new(
                        GameData {
                            resolution: (800, 600),
                            mouse_pos: (0.0, 0.0),
                            profile_stack: vec![],
                            input_state: InputState::clone(&sp.get_required()),
                            audio_samples: Default::default(),
                            audio_sample_play_status: Default::default(),
                        }
                        .into(),
                    )
                },
            ),
            lifetime,
        )
    }
}
pub struct GameDataLua;

#[allow(non_snake_case)]
#[mlua_bridge::mlua_bridge]
impl GameDataLua {
    //GetMousePos
    fn GetMousePos(game_data: &RefMut<GameData>) -> mlua::Result<(f64, f64)> {
        Ok(game_data.read().expect("Lock error").mouse_pos)
    }

    fn GetResolution(game_data: &RefMut<GameData>) -> mlua::Result<(u32, u32)> {
        Ok(game_data.read().expect("Lock error").resolution)
    }

    /*
       Debug = 0,
       Info = 1,
       Normal = 2,
       Warning = 3,
       Error = 4
    */

    fn Log(lua: &LuaKey, message: String, severity: i32) {
        use log::*;
        let d = "Lua";
        log!(
            target: &d,
            match severity {
                0 => Level::Debug,
                1 => Level::Info,
                2 => Level::Info,
                3 => Level::Warn,
                4 => Level::Error,
                _ => Level::Debug,
            },
            "{}",
            message
        );
    }

    fn LoadSkinSample(game_data: &RefMut<GameData>, name: String) -> mlua::Result<()> {
        let mut gd_lock = game_data.write().expect("Lock error");
        let game_data = gd_lock.deref_mut();
        if game_data.audio_samples.contains_key(&name) {
            return Ok(());
        }
        let config = GameConfig::get();

        let mut folder = config.game_folder.clone();
        folder.push("skins");
        folder.push(&config.skin);
        folder.push("audio");
        folder.push(&name);
        if folder.extension().is_none() {
            folder.set_extension("wav");
        }

        let file = std::fs::File::open(&folder).map_err(mlua::Error::external)?;

        let decoder = rodio::Decoder::new(file)
            .map_err(mlua::Error::external)?
            .buffered();

        game_data.audio_samples.insert(name, decoder);

        Ok(())
    }

    fn PlaySample(
        game_data: &RefMut<GameData>,
        mixer: &RuscMixer,
        name: String,
        do_loop: bool,
    ) -> mlua::Result<()> {
        let mut gd_lock = game_data.write().expect("Lock error");
        let game_data = gd_lock.deref_mut();
        let Some(sample) = game_data.audio_samples.get(&name) else {
            warn!("No sample named: {name}");
            return Ok(());
        };

        let play_control = Arc::new(AtomicUsize::new(1));
        let prev = game_data
            .audio_sample_play_status
            .insert(name, play_control.clone());

        if let Some(p) = prev {
            p.store(0, std::sync::atomic::Ordering::SeqCst);
        }

        let to_play = sample.clone();
        if do_loop {
            mixer.add(
                to_play
                    .convert_samples()
                    .repeat_infinite()
                    .stoppable()
                    .periodic_access(Duration::from_millis(10), move |x| {
                        if play_control.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                            x.stop()
                        }
                    }),
            )
        } else {
            let done_control = play_control.clone();
            mixer.add(rodio::source::Done::new(
                to_play.convert_samples().stoppable().periodic_access(
                    Duration::from_millis(10),
                    move |x| {
                        if play_control.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                            x.stop()
                        }
                    },
                ),
                done_control,
            ))
        }

        Ok(())
    }

    fn StopSample(game_data: &RefMut<GameData>, name: String) -> mlua::Result<()> {
        game_data
            .write()
            .expect("Lock Error")
            .audio_sample_play_status
            .entry(name)
            .and_modify(|x| x.store(0, std::sync::atomic::Ordering::SeqCst));

        Ok(())
    }

    fn IsSamplePlaying(game_data: &RefMut<GameData>, name: String) -> mlua::Result<Option<bool>> {
        let game_data = game_data.read().expect("Lock error");
        if !game_data.audio_samples.contains_key(&name) {
            return Ok(None);
        }

        match game_data.audio_sample_play_status.get(&name) {
            Some(a) => Ok(Some(a.load(std::sync::atomic::Ordering::SeqCst) == 1)),
            None => Ok(Some(false)),
        }
    }

    fn GetLaserColor(laser: i32) -> mlua::Result<(f32, f32, f32, f32)> {
        if let Some(hue) = GameConfig::get().laser_hues.get(laser as usize).copied() {
            let [r, g, b] = Hsva::new(hue / 360.0, 1.0, 1.0, 1.0).to_rgb();
            Ok((r * 255.0, g * 255.0, b * 255.0, 255.0))
        } else {
            Err(mlua::Error::external("Bad laser index"))
        }
    }

    fn GetButton(game_data: &RefMut<GameData>, button: u8) -> mlua::Result<bool> {
        let game_data = game_data.read().expect("Lock error");
        Ok(game_data
            .input_state
            .is_button_held(UscButton::from(button))
            .is_some())
    }

    fn GetKnob(game_data: &RefMut<GameData>, knob: i32) -> mlua::Result<f32> {
        let game_data = game_data.read().expect("Lock error");
        match knob {
            0 => Ok(game_data.input_state.get_axis(kson::Side::Left).pos),
            1 => Ok(game_data.input_state.get_axis(kson::Side::Right).pos),
            _ => Err(mlua::Error::RuntimeError(format!(
                "Invalid laser index: {}",
                knob
            ))),
        }
    }

    fn UpdateAvailable() -> mlua::Result<()> {
        Ok(())
    }

    fn GetSkin() -> mlua::Result<String> {
        Ok(GameConfig::get().skin.clone())
    }

    fn GetSkinSetting(key: String) -> mlua::Result<SkinSettingValue> {
        let skin_setting_value = GameConfig::get()
            .skin_settings
            .get(&key)
            .cloned()
            .unwrap_or(SkinSettingValue::None);

        Ok(skin_setting_value)
    }

    fn SetSkinSetting(key: (String, SkinSettingValue)) -> mlua::Result<()> {
        GameConfig::get_mut().skin_settings.insert(key.0, key.1);

        Ok(())
    }

    fn BeginProfile(
        lua: &LuaKey,
        scope: Option<String>,
        game_data: &RefMut<GameData>,
    ) -> mlua::Result<()> {
        let mut gd_lock = game_data.write().expect("Lock error");
        let game_data = gd_lock.deref_mut();

        let custom_scope =
            ThreadProfiler::call(|f| f.register_function_scope("Custom Lua Scope", "", 0));

        if puffin::are_scopes_on() {
            let scope = "Unknown";

            game_data
                .profile_stack
                .push(ProfilerScope::new(custom_scope, scope))
        }
        Ok(())
    }

    //EndProfile
    fn EndProfile(game_data: &RefMut<GameData>) {
        let mut gd_lock = game_data.write().expect("Lock error");
        let game_data = gd_lock.deref_mut();
        game_data.profile_stack.pop();
    }

    const LOGGER_INFO: u8 = 0;
    const LOGGER_NORMAL: u8 = 1;
    const LOGGER_WARNING: u8 = 2;
    const LOGGER_ERROR: u8 = 3;

    const BUTTON_BTA: u8 = 0;
    const BUTTON_BTB: u8 = 1;
    const BUTTON_BTC: u8 = 2;
    const BUTTON_BTD: u8 = 3;
    const BUTTON_FXL: u8 = 4;
    const BUTTON_FXR: u8 = 5;
    const BUTTON_STA: u8 = 6;
    const BUTTON_BCK: u8 = 7;
}

#[derive(Default)]
pub struct LuaPath;

impl UserData for LuaPath {
    fn add_methods<T: UserDataMethods<Self>>(_methods: &mut T) {
        _methods.add_function("Absolute", |_, s: String| {
            let mut p = GameConfig::get().game_folder.clone();
            p.push(s);
            Ok(p.to_string_lossy().to_string())
        })
    }
}

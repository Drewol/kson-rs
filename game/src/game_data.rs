use std::{
    cell::Ref,
    collections::HashMap,
    path::PathBuf,
    sync::{atomic::AtomicUsize, Arc},
    time::Duration,
};

use egui::epaint::Hsva;
use log::warn;
use puffin::ProfilerScope;
use rodio::Source;
use tealr::{
    mlu::{
        mlua::{self},
        UserData,
    },
    mlu::{TealData, UserDataProxy},
    TypeName,
};

use crate::{
    button_codes::UscButton, config::GameConfig, help::add_lua_static_method,
    input_state::InputState, skin_settings::SkinSettingValue, RuscMixer,
};

#[derive(UserData)]
pub struct GameData {
    pub resolution: (u32, u32),
    pub mouse_pos: (f64, f64),
    pub profile_stack: Vec<ProfilerScope>,
    pub input_state: InputState,
    pub audio_samples: HashMap<String, rodio::source::Buffered<rodio::Decoder<std::fs::File>>>,
    pub audio_sample_play_status: HashMap<String, Arc<AtomicUsize>>,
}

impl TypeName for GameData {
    fn get_type_parts() -> std::borrow::Cow<'static, [tealr::NamePart]> {
        use std::borrow::Cow;

        Cow::Borrowed(&[tealr::NamePart::Type(tealr::TealType {
            name: Cow::Borrowed("game"),
            type_kind: tealr::KindOfType::External,
            generics: None,
        })])
    }
}

impl TealData for GameData {
    fn add_methods<'lua, T: tealr::mlu::TealDataMethods<'lua, Self>>(methods: &mut T) {
        //GetMousePos
        add_lua_static_method(methods, "GetMousePos", |_, _game_data, _: ()| {
            Ok(_game_data.mouse_pos)
        });

        //GetResolution
        add_lua_static_method(methods, "GetResolution", |_, _game_data, _: ()| {
            Ok(_game_data.resolution)
        });

        //Log

        /*
           Debug = 0,
           Info = 1,
           Normal = 2,
           Warning = 3,
           Error = 4
        */
        tealr::mlu::create_named_parameters!(LogParams with
          message : String,
          severity : i32,

        );
        add_lua_static_method(methods, "Log", |lua, _game_data, p: LogParams| {
            use log::*;
            let LogParams { message, severity } = p;
            let d = lua
                .inspect_stack(1)
                .and_then(|x| {
                    x.source()
                        .short_src
                        .map(String::from_utf8_lossy)
                        .map(|s| s.to_string())
                        .map(PathBuf::from)
                        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
                })
                .unwrap_or_else(|| String::from("Unknown"));
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

            Ok(())
        });

        //LoadSkinSample
        tealr::mlu::create_named_parameters!(LoadSkinSampleParams with
          name : String,

        );
        add_lua_static_method(
            methods,
            "LoadSkinSample",
            |_, game_data, p: LoadSkinSampleParams| {
                let LoadSkinSampleParams { name } = p;

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

                let file =
                    std::fs::File::open(&folder).map_err(tealr::mlu::mlua::Error::external)?;

                let decoder = rodio::Decoder::new(file)
                    .map_err(tealr::mlu::mlua::Error::external)?
                    .buffered();

                game_data.audio_samples.insert(name, decoder);

                Ok(())
            },
        );

        //PlaySample
        tealr::mlu::create_named_parameters!(PlaySampleParams with
          name : String,
          do_loop : bool,

        );
        add_lua_static_method(
            methods,
            "PlaySample",
            |lua, game_data, p: PlaySampleParams| {
                let PlaySampleParams { name, do_loop } = p;

                let sample = game_data.audio_samples.get(&name);

                if sample.is_none() {
                    warn!("No sample named: {name}");
                    return Ok(());
                }

                let mixer: Ref<RuscMixer> = lua.app_data_ref().unwrap();

                let play_control = Arc::new(AtomicUsize::new(1));
                let prev = game_data
                    .audio_sample_play_status
                    .insert(name, play_control.clone());

                if let Some(p) = prev {
                    p.store(0, std::sync::atomic::Ordering::SeqCst);
                }

                let to_play = sample.unwrap().clone();
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
            },
        );

        //StopSample
        tealr::mlu::create_named_parameters!(StopSampleParams with
          name : String,

        );
        add_lua_static_method(
            methods,
            "StopSample",
            |_, game_data, p: StopSampleParams| {
                let StopSampleParams { name } = p;

                game_data
                    .audio_sample_play_status
                    .entry(name)
                    .and_modify(|x| x.store(0, std::sync::atomic::Ordering::SeqCst));

                Ok(())
            },
        );

        //IsSamplePlaying
        tealr::mlu::create_named_parameters!(IsSamplePlayingParams with
          name : String,

        );
        add_lua_static_method(
            methods,
            "IsSamplePlaying",
            |_, game_data, p: IsSamplePlayingParams| {
                let IsSamplePlayingParams { name } = p;
                if !game_data.audio_samples.contains_key(&name) {
                    return Ok(None);
                }

                match game_data.audio_sample_play_status.get(&name) {
                    Some(a) => Ok(Some(a.load(std::sync::atomic::Ordering::SeqCst) == 1)),
                    None => Ok(Some(false)),
                }
            },
        );

        //GetLaserColor
        tealr::mlu::create_named_parameters!(GetLaserColorParams with
          laser : i32,

        );
        add_lua_static_method(
            methods,
            "GetLaserColor",
            |_, _game_data, _p: GetLaserColorParams| {
                if let Some(hue) = GameConfig::get().laser_hues.get(_p.laser as usize).copied() {
                    let [r, g, b] = Hsva::new(hue / 360.0, 1.0, 1.0, 1.0).to_rgb();
                    Ok((r * 255.0, g * 255.0, b * 255.0, 255.0))
                } else {
                    Err(mlua::Error::external("Bad laser index"))
                }
            },
        );

        //GetButton
        tealr::mlu::create_named_parameters!(GetButtonParams with
          button : u8,

        );
        add_lua_static_method(
            methods,
            "GetButton",
            |_, game_data, GetButtonParams { button }: GetButtonParams| {
                Ok(game_data
                    .input_state
                    .is_button_held(UscButton::from(button))
                    .is_some())
            },
        );

        //GetKnob
        tealr::mlu::create_named_parameters!(GetKnobParams with
          knob : i32,

        );
        add_lua_static_method(
            methods,
            "GetKnob",
            |_, game_data, p: GetKnobParams| match p.knob {
                0 => Ok(game_data.input_state.get_axis(kson::Side::Left).pos),
                1 => Ok(game_data.input_state.get_axis(kson::Side::Right).pos),
                _ => Err(mlua::Error::RuntimeError(format!(
                    "Invalid laser index: {}",
                    p.knob
                ))),
            },
        );

        //UpdateAvailable
        add_lua_static_method(methods, "UpdateAvailable", |_, _game_data, _: ()| Ok(()));

        //GetSkin
        add_lua_static_method(methods, "GetSkin", |_, _game_data, _: ()| {
            Ok(GameConfig::get().skin.clone())
        });

        //GetSkinSetting
        add_lua_static_method(methods, "GetSkinSetting", |_, _game_data, key: String| {
            let skin_setting_value = GameConfig::get()
                .skin_settings
                .get(&key)
                .cloned()
                .unwrap_or(SkinSettingValue::None);

            Ok(skin_setting_value)
        });

        //GetSkinSetting
        add_lua_static_method(
            methods,
            "SetSkinSetting",
            |_, _game_data, key: (String, SkinSettingValue)| {
                GameConfig::get_mut().skin_settings.insert(key.0, key.1);

                Ok(())
            },
        );

        //BeginProfile
        add_lua_static_method(
            methods,
            "BeginProfile",
            |lua, _game_data, scope: Option<String>| {
                if puffin::are_scopes_on() {
                    let scope = scope.unwrap_or_else(|| {
                        if let Some(a) = lua.inspect_stack(1) {
                            let names = a.names();
                            names
                                .name
                                .map(|a| String::from_utf8_lossy(a).to_string())
                                .unwrap_or_else(|| "unknown".to_string())
                        } else {
                            "unknown".to_string()
                        }
                    });

                    _game_data
                        .profile_stack
                        .push(ProfilerScope::new("Lua scope", &scope, ""))
                }
                Ok(())
            },
        );

        //EndProfile
        add_lua_static_method(methods, "EndProfile", |_, _game_data, _: ()| {
            _game_data.profile_stack.pop();
            Ok(())
        })
    }

    fn add_fields<'lua, F: tealr::mlu::TealDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_function_get("LOGGER_INFO", |_, _| Ok(0));
        fields.add_field_function_get("LOGGER_NORMAL", |_, _| Ok(1));
        fields.add_field_function_get("LOGGER_WARNING", |_, _| Ok(2));
        fields.add_field_function_get("LOGGER_ERROR", |_, _| Ok(3));
        fields.add_field_function_get("BUTTON_BTA", |_, _| Ok(0));
        fields.add_field_function_get("BUTTON_BTB", |_, _| Ok(1));
        fields.add_field_function_get("BUTTON_BTC", |_, _| Ok(2));
        fields.add_field_function_get("BUTTON_BTD", |_, _| Ok(3));
        fields.add_field_function_get("BUTTON_FXL", |_, _| Ok(4));
        fields.add_field_function_get("BUTTON_FXR", |_, _| Ok(5));
        fields.add_field_function_get("BUTTON_STA", |_, _| Ok(6));
        fields.add_field_function_get("BUTTON_BCK", |_, _| Ok(7));
    }
}

// document and expose the global proxy
#[derive(Default)]
pub struct ExportGame;
impl tealr::mlu::ExportInstances for ExportGame {
    fn add_instances<'lua, T: tealr::mlu::InstanceCollector<'lua>>(
        self,
        instance_collector: &mut T,
    ) -> mlua::Result<()> {
        instance_collector.document_instance("Documentation for the exposed static proxy");

        // note that the proxy type is NOT `Example` but a special mlua type, which is represented differnetly in .d.tl as well
        instance_collector.add_instance("game", UserDataProxy::<GameData>::new)?;
        Ok(())
    }
}

#[derive(UserData, TypeName, Default)]
pub struct LuaPath;

impl TealData for LuaPath {
    fn add_methods<'lua, T: tealr::mlu::TealDataMethods<'lua, Self>>(_methods: &mut T) {
        _methods.add_function("Absolute", |_, s: String| {
            let mut p = GameConfig::get().game_folder.clone();
            p.push(s);
            Ok(p.to_string_lossy().to_string())
        })
    }

    fn add_fields<'lua, F: tealr::mlu::TealDataFields<'lua, Self>>(_fields: &mut F) {}
}
impl tealr::mlu::ExportInstances for LuaPath {
    fn add_instances<'lua, T: tealr::mlu::InstanceCollector<'lua>>(
        self,
        instance_collector: &mut T,
    ) -> mlua::Result<()> {
        instance_collector.add_instance("path", UserDataProxy::<Self>::new)?;
        Ok(())
    }
}

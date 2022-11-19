use std::{
    cell::RefMut,
    sync::{Arc, Mutex},
};

use tealr::{
    mlu::{
        mlua::{self, FromLuaMulti, Lua, Result, ToLuaMulti},
        MaybeSend, TealDataMethods, UserData,
    },
    mlu::{TealData, UserDataProxy},
    TealMultiValue, TypeName,
};

use crate::help::add_lua_static_method;

#[derive(Debug, UserData, Clone, Copy)]
pub struct GameData {
    pub resolution: (u32, u32),
    pub mouse_pos: (f64, f64),
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
        add_lua_static_method(methods, "Log", |_, _game_data, p: LogParams| {
            use log::*;
            let LogParams { message, severity } = p;
            match severity {
                0 => debug!("{}", message),
                1 => info!("{}", message),
                2 => info!("{}", message),
                3 => warn!("{}", message),
                4 => error!("{}", message),
                _ => {}
            }

            Ok(())
        });

        //LoadSkinSample
        tealr::mlu::create_named_parameters!(LoadSkinSampleParams with
          name : String,

        );
        add_lua_static_method(
            methods,
            "LoadSkinSample",
            |_, _game_data, p: LoadSkinSampleParams| Ok(()),
        );

        //PlaySample
        tealr::mlu::create_named_parameters!(PlaySampleParams with
          name : String,
          doLoop : bool,

        );
        add_lua_static_method(
            methods,
            "PlaySample",
            |_, _game_data, p: PlaySampleParams| Ok(()),
        );

        //StopSample
        tealr::mlu::create_named_parameters!(StopSampleParams with
          name : String,

        );
        add_lua_static_method(
            methods,
            "StopSample",
            |_, _game_data, p: StopSampleParams| Ok(()),
        );

        //IsSamplePlaying
        tealr::mlu::create_named_parameters!(IsSamplePlayingParams with
          name : String,

        );
        add_lua_static_method(
            methods,
            "IsSamplePlaying",
            |_, _game_data, p: IsSamplePlayingParams| Ok(false),
        );

        //GetLaserColor
        tealr::mlu::create_named_parameters!(GetLaserColorParams with
          laser : i32,

        );
        add_lua_static_method(
            methods,
            "GetLaserColor",
            |_, _game_data, p: GetLaserColorParams| Ok((0, 127, 255, 255)),
        );

        //GetButton
        tealr::mlu::create_named_parameters!(GetButtonParams with
          button : i32,

        );
        add_lua_static_method(methods, "GetButton", |_, _game_data, p: GetButtonParams| {
            Ok(false)
        });

        //GetKnob
        tealr::mlu::create_named_parameters!(GetKnobParams with
          knob : i32,

        );
        add_lua_static_method(methods, "GetKnob", |_, _game_data, p: GetKnobParams| {
            Ok(0.5)
        });

        //UpdateAvailable
        add_lua_static_method(methods, "UpdateAvailable", |_, _game_data, _: ()| Ok(()));

        //GetSkin
        add_lua_static_method(methods, "GetSkin", |_, _game_data, _: ()| Ok("default"));

        //GetSkinSetting
        add_lua_static_method(methods, "GetSkinSetting", |_, _game_data, key: (String)| {
            Ok((0, 127, 255, 255))
        });
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

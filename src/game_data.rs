use tealr::{
    mlu::{mlua, UserData},
    mlu::{TealData, UserDataProxy},
    TypeName,
};

#[derive(Debug, TypeName, UserData)]
pub struct GameData {
    pub resolution: (u32, u32),
    pub mouse_pos: (f64, f64),
}

impl TealData for GameData {
    fn add_methods<'lua, T: tealr::mlu::TealDataMethods<'lua, Self>>(methods: &mut T) {
        //GetMousePos
        methods.add_method("GetMousePos", |_, _game_data, _: ()| {
            Ok(_game_data.mouse_pos)
        });

        //GetResolution
        methods.add_method("GetResolution", |_, _game_data, _: ()| {
            Ok(_game_data.resolution)
        });

        //Log
        tealr::mlu::create_named_parameters!(LogParams with
          message : String,
          severity : i32,

        );
        methods.add_method("Log", |_, _game_data, p: LogParams| {
            println!("{}", p.message);
            Ok(())
        });

        //LoadSkinSample
        tealr::mlu::create_named_parameters!(LoadSkinSampleParams with
          name : String,

        );
        methods.add_method(
            "LoadSkinSample",
            |_, _game_data, p: LoadSkinSampleParams| Ok(()),
        );

        //PlaySample
        tealr::mlu::create_named_parameters!(PlaySampleParams with
          name : String,
          doLoop : bool,

        );
        methods.add_method("PlaySample", |_, _game_data, p: PlaySampleParams| Ok(()));

        //StopSample
        tealr::mlu::create_named_parameters!(StopSampleParams with
          name : String,

        );
        methods.add_method("StopSample", |_, _game_data, p: StopSampleParams| Ok(()));

        //IsSamplePlaying
        tealr::mlu::create_named_parameters!(IsSamplePlayingParams with
          name : String,

        );
        methods.add_method(
            "IsSamplePlaying",
            |_, _game_data, p: IsSamplePlayingParams| Ok(false),
        );

        //GetLaserColor
        tealr::mlu::create_named_parameters!(GetLaserColorParams with
          laser : i32,

        );
        methods.add_method("GetLaserColor", |_, _game_data, p: GetLaserColorParams| {
            Ok((0, 127, 255, 255))
        });

        //GetButton
        tealr::mlu::create_named_parameters!(GetButtonParams with
          button : i32,

        );
        methods.add_method("GetButton", |_, _game_data, p: GetButtonParams| Ok(false));

        //GetKnob
        tealr::mlu::create_named_parameters!(GetKnobParams with
          knob : i32,

        );
        methods.add_method("GetKnob", |_, _game_data, p: GetKnobParams| Ok(0.5));

        //UpdateAvailable
        methods.add_method("UpdateAvailable", |_, _game_data, _: ()| Ok(()));

        //GetSkin
        methods.add_method("GetSkin", |_, _game_data, _: ()| Ok("default"));

        //GetSkinSetting
        methods.add_method("GetSkinSetting", |_, _game_data, key: (String)| {
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
        instance_collector.add_instance("GameData", UserDataProxy::<GameData>::new)?;
        Ok(())
    }
}

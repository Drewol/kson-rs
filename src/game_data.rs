use std::cell::RefCell;

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

thread_local! {
  static INSTANCE: RefCell<GameData> = RefCell::new(GameData { resolution: (1,1), mouse_pos: (0.0, 0.0) });
}

pub fn set_game_data(data: GameData) {
    INSTANCE.with(|a| {
        a.replace(data);
    })
}

pub fn with_game_data<A>(f: impl Fn(&GameData) -> A) -> A {
    INSTANCE.with(|instance| {
        let borrowed = instance.borrow();
        f(&borrowed)
    })
}

impl TealData for GameData {
    fn add_methods<'lua, T: tealr::mlu::TealDataMethods<'lua, Self>>(methods: &mut T) {
        //GetMousePos
        methods.add_function("GetMousePos", |_, _: ()| {
            Ok(with_game_data(|d| d.mouse_pos))
        });

        //GetResolution
        methods.add_function("GetResolution", |_, _: ()| {
            Ok(with_game_data(|d| d.resolution))
        });

        //Log
        tealr::mlu::create_named_parameters!(LogParams with
          message : String,
          severity : i32,

        );
        methods.add_function("Log", |_, p: LogParams| {
            println!("{}", p.message);
            Ok(())
        });

        //LoadSkinSample
        tealr::mlu::create_named_parameters!(LoadSkinSampleParams with
          name : String,

        );
        methods.add_function("LoadSkinSample", |_, p: LoadSkinSampleParams| Ok(()));

        //PlaySample
        tealr::mlu::create_named_parameters!(PlaySampleParams with
          name : String,
          doLoop : bool,

        );
        methods.add_function("PlaySample", |_, p: PlaySampleParams| Ok(()));

        //StopSample
        tealr::mlu::create_named_parameters!(StopSampleParams with
          name : String,

        );
        methods.add_function("StopSample", |_, p: StopSampleParams| Ok(()));

        //IsSamplePlaying
        tealr::mlu::create_named_parameters!(IsSamplePlayingParams with
          name : String,

        );
        methods.add_function("IsSamplePlaying", |_, p: IsSamplePlayingParams| Ok(false));

        //GetLaserColor
        tealr::mlu::create_named_parameters!(GetLaserColorParams with
          laser : i32,

        );
        methods.add_function("GetLaserColor", |_, p: GetLaserColorParams| {
            Ok((0, 127, 255, 255))
        });

        //GetButton
        tealr::mlu::create_named_parameters!(GetButtonParams with
          button : i32,

        );
        methods.add_function("GetButton", |_, p: GetButtonParams| Ok(false));

        //GetKnob
        tealr::mlu::create_named_parameters!(GetKnobParams with
          knob : i32,

        );
        methods.add_function("GetKnob", |_, p: GetKnobParams| Ok(0.5));

        //UpdateAvailable
        methods.add_function("UpdateAvailable", |_, _: ()| Ok(()));

        //GetSkin
        methods.add_function("GetSkin", |_, _: ()| Ok("default"));

        //GetSkinSetting
        methods.add_function("GetSkinSetting", |_, key: (String)| Ok((0, 127, 255, 255)));
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
        instance_collector.add_instance("GameData", UserDataProxy::<GameData>::new)?;
        Ok(())
    }
}

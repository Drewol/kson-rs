use std::{
    cell::Ref,
    sync::{
        mpsc::{Receiver, Sender},
        Arc, Mutex,
    },
};

use tealr::{
    mlu::{
        mlua::{Function, Lua},
        ExportInstances, TealData, UserData, UserDataProxy,
    },
    TypeName,
};

use crate::scene::Scene;
#[derive(Debug)]
enum Buttons {
    Start,
    Downloads,
    Multiplayer,
    Options,
    Exit,
    Update,
    Challenges,
}

#[derive(Debug, UserData, TypeName)]
struct Bindings(Sender<Buttons>);

impl TealData for Bindings {
    fn add_methods<'lua, T: tealr::mlu::TealDataMethods<'lua, Self>>(methods: &mut T) {
        use tealr::mlu::mlua::Error;

        /*
        m_luaBinds->AddFunction("Start", this, &TitleScreen_Impl::lStart);
        m_luaBinds->AddFunction("DLScreen", this, &TitleScreen_Impl::lDownloads);
        m_luaBinds->AddFunction("Exit", this, &TitleScreen_Impl::lExit);
        m_luaBinds->AddFunction("Settings", this, &TitleScreen_Impl::lSettings);
        m_luaBinds->AddFunction("Multiplayer", this, &TitleScreen_Impl::lMultiplayer);
        m_luaBinds->AddFunction("Challenges", this, &TitleScreen_Impl::lChallengeSelect);

        m_luaBinds->AddFunction("Update", this, &TitleScreen_Impl::lUpdate);
         */

        methods.add_function("Start", |lua, ()| {
            let s: Ref<Sender<Buttons>> = lua.app_data_ref().unwrap();
            s.send(Buttons::Start).map_err(Error::external)
        });
        methods.add_function("DLScreen", |lua, ()| {
            let s: Ref<Sender<Buttons>> = lua.app_data_ref().unwrap();
            s.send(Buttons::Downloads).map_err(Error::external)
        });
        methods.add_function("Multiplayer", |lua, ()| {
            let s: Ref<Sender<Buttons>> = lua.app_data_ref().unwrap();
            s.send(Buttons::Multiplayer).map_err(Error::external)
        });
        methods.add_function("Exit", |lua, ()| {
            let s: Ref<Sender<Buttons>> = lua.app_data_ref().unwrap();
            s.send(Buttons::Exit).map_err(Error::external)
        });
        methods.add_function("Settings", |lua, ()| {
            let s: Ref<Sender<Buttons>> = lua.app_data_ref().unwrap();
            s.send(Buttons::Options).map_err(Error::external)
        });
        methods.add_function("Update", |lua, ()| {
            let s: Ref<Sender<Buttons>> = lua.app_data_ref().unwrap();
            s.send(Buttons::Update).map_err(Error::external)
        });
        methods.add_function("Challenges", |lua, ()| {
            let s: Ref<Sender<Buttons>> = lua.app_data_ref().unwrap();
            s.send(Buttons::Challenges).map_err(Error::external)
        });
    }
}

#[derive(Debug, Default)]
struct ExportBindings;
impl ExportInstances for ExportBindings {
    fn add_instances<'lua, T: tealr::mlu::InstanceCollector<'lua>>(
        self,
        instance_collector: &mut T,
    ) -> tealr::mlu::mlua::Result<()> {
        instance_collector.add_instance("Menu", UserDataProxy::<Bindings>::new)?;
        Ok(())
    }
}

pub struct MainMenu {
    lua: Lua,
    button_rx: Receiver<Buttons>,
}

impl MainMenu {
    pub fn new() -> Self {
        let lua = Lua::new();
        let (tx, button_rx) = std::sync::mpsc::channel();
        lua.set_app_data(tx);
        tealr::mlu::set_global_env(ExportBindings, &lua);
        Self { lua, button_rx }
    }
}

impl Scene for MainMenu {
    fn render(&mut self, dt: f64) -> anyhow::Result<bool> {
        let render: Function = self.lua.globals().get("render")?;
        render.call(dt / 1000.0)?;
        Ok(false)
    }

    fn init(
        &mut self,
        load_lua: Box<dyn Fn(&Lua, &'static str) -> anyhow::Result<()>>,
    ) -> anyhow::Result<()> {
        load_lua(&self.lua, "titlescreen.lua")?;
        Ok(())
    }

    fn tick(&mut self, dt: f64, game_data: crate::game_data::GameData) -> anyhow::Result<bool> {
        self.lua.globals().set("game", game_data)?;

        while let Ok(button) = self.button_rx.try_recv() {
            log::info!("Pressed: {:?}", &button);
            if let Buttons::Exit = button {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn on_event(&mut self, event: &mut three_d::Event) {
        if let three_d::Event::MousePress {
            button,
            position,
            modifiers,
            handled,
        } = event
        {
            if let Ok(mouse_pressed) = self.lua.globals().get::<_, Function>("mouse_pressed") {
                if let Err(e) = mouse_pressed.call::<_, ()>(*button as u8) {
                    log::error!("{:?}", e);
                };
            }
        }
    }

    fn is_suspended(&self) -> bool {
        false
    }

    fn debug_ui(&mut self, ctx: &three_d::egui::Context) -> anyhow::Result<()> {
        Ok(())
    }
}

use std::{
    cell::Ref,
    rc::Rc,
    sync::{
        mpsc::{Receiver, Sender},
        Arc, Mutex,
    },
};

use anyhow::{anyhow, Result};
use generational_arena::Index;
use tealr::{
    mlu::{
        mlua::{Function, Lua},
        ExportInstances, TealData, UserData, UserDataProxy,
    },
    TypeName,
};

use crate::{button_codes::LaserState, scene::Scene, ControlMessage};
#[derive(Debug, Clone, Copy)]
pub enum MainMenuButton {
    Start,
    Downloads,
    Multiplayer,
    Options,
    Exit,
    Update,
    Challenges,
}

#[derive(Debug, UserData, TypeName)]
struct Bindings(Sender<MainMenuButton>);

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
            let s: Ref<Sender<MainMenuButton>> = lua.app_data_ref().unwrap();
            s.send(MainMenuButton::Start).map_err(Error::external)
        });
        methods.add_function("DLScreen", |lua, ()| {
            let s: Ref<Sender<MainMenuButton>> = lua.app_data_ref().unwrap();
            s.send(MainMenuButton::Downloads).map_err(Error::external)
        });
        methods.add_function("Multiplayer", |lua, ()| {
            let s: Ref<Sender<MainMenuButton>> = lua.app_data_ref().unwrap();
            s.send(MainMenuButton::Multiplayer).map_err(Error::external)
        });
        methods.add_function("Exit", |lua, ()| {
            let s: Ref<Sender<MainMenuButton>> = lua.app_data_ref().unwrap();
            s.send(MainMenuButton::Exit).map_err(Error::external)
        });
        methods.add_function("Settings", |lua, ()| {
            let s: Ref<Sender<MainMenuButton>> = lua.app_data_ref().unwrap();
            s.send(MainMenuButton::Options).map_err(Error::external)
        });
        methods.add_function("Update", |lua, ()| {
            let s: Ref<Sender<MainMenuButton>> = lua.app_data_ref().unwrap();
            s.send(MainMenuButton::Update).map_err(Error::external)
        });
        methods.add_function("Challenges", |lua, ()| {
            let s: Ref<Sender<MainMenuButton>> = lua.app_data_ref().unwrap();
            s.send(MainMenuButton::Challenges).map_err(Error::external)
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
    lua: Rc<Lua>,
    button_rx: Receiver<MainMenuButton>,
    control_tx: Option<Sender<ControlMessage>>,
    should_suspended: bool,
    suspended: bool,
}

impl MainMenu {
    pub fn new() -> Self {
        let lua = Rc::new(Lua::new());
        let (tx, button_rx) = std::sync::mpsc::channel();
        lua.set_app_data(tx);
        tealr::mlu::set_global_env(ExportBindings, &lua);
        Self {
            lua,
            button_rx,
            control_tx: None,
            suspended: false,
            should_suspended: false,
        }
    }
}

impl Scene for MainMenu {
    fn render_ui(&mut self, dt: f64) -> anyhow::Result<()> {
        let render: Function = self.lua.globals().get("render")?;
        render.call(dt / 1000.0)?;
        Ok(())
    }

    fn init(
        &mut self,
        load_lua: Box<dyn Fn(Rc<Lua>, &'static str) -> anyhow::Result<Index>>,
        app_control_tx: Sender<ControlMessage>,
    ) -> anyhow::Result<()> {
        load_lua(self.lua.clone(), "titlescreen.lua")?;
        self.control_tx = Some(app_control_tx);
        Ok(())
    }

    fn tick(&mut self, dt: f64, knob_state: LaserState) -> Result<()> {
        if self.should_suspended {
            self.suspended = true;
            self.should_suspended = false;
        }

        while let Ok(button) = self.button_rx.try_recv() {
            log::info!("Pressed: {:?}", &button);
            self.control_tx
                .as_ref()
                .unwrap()
                .send(ControlMessage::MainMenu(button))
                .map_err(|_| anyhow!("Failed to send button"))?;
        }

        Ok(())
    }

    fn on_event(&mut self, event: &mut three_d::Event<()>) {
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

    fn suspend(&mut self) {
        self.should_suspended = true;
    }

    fn is_suspended(&self) -> bool {
        self.suspended
    }

    fn resume(&mut self) {
        self.suspended = false;
    }

    fn debug_ui(&mut self, ctx: &three_d::egui::Context) -> anyhow::Result<()> {
        Ok(())
    }

    fn closed(&self) -> bool {
        false
    }

    fn name(&self) -> &str {
        "Main Menu"
    }
}

use std::{
    rc::Rc,
    sync::mpsc::{Receiver, Sender},
    time::SystemTime,
};

use anyhow::{anyhow, Result};
use di::ServiceProvider;
use game_loop::winit::event::{ElementState, Event, WindowEvent};
use tealr::{
    mlu::{
        mlua::{self, AppDataRef, Function, Lua},
        ExportInstances, TealData, UserData, UserDataProxy,
    },
    ToTypename,
};

use crate::{
    button_codes::{LaserState, UscInputEvent},
    lua_service::LuaProvider,
    scene::Scene,
    ControlMessage,
};
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

#[derive(Debug, UserData, ToTypename)]
struct Bindings;

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
            let s: AppDataRef<Sender<MainMenuButton>> = lua
                .app_data_ref()
                .ok_or(mlua::Error::external("Button app data not set"))?;
            s.send(MainMenuButton::Start).map_err(Error::external)
        });
        methods.add_function("DLScreen", |lua, ()| {
            let s: AppDataRef<Sender<MainMenuButton>> = lua
                .app_data_ref()
                .ok_or(mlua::Error::external("Button app data not set"))?;
            s.send(MainMenuButton::Downloads).map_err(Error::external)
        });
        methods.add_function("Multiplayer", |lua, ()| {
            let s: AppDataRef<Sender<MainMenuButton>> = lua
                .app_data_ref()
                .ok_or(mlua::Error::external("Button app data not set"))?;
            s.send(MainMenuButton::Multiplayer).map_err(Error::external)
        });
        methods.add_function("Exit", |lua, ()| {
            let s: AppDataRef<Sender<MainMenuButton>> = lua
                .app_data_ref()
                .ok_or(mlua::Error::external("Button app data not set"))?;
            s.send(MainMenuButton::Exit).map_err(Error::external)
        });
        methods.add_function("Settings", |lua, ()| {
            let s: AppDataRef<Sender<MainMenuButton>> = lua
                .app_data_ref()
                .ok_or(mlua::Error::external("Button app data not set"))?;
            s.send(MainMenuButton::Options).map_err(Error::external)
        });
        methods.add_function("Update", |lua, ()| {
            let s: AppDataRef<Sender<MainMenuButton>> = lua
                .app_data_ref()
                .ok_or(mlua::Error::external("Button app data not set"))?;
            s.send(MainMenuButton::Update).map_err(Error::external)
        });
        methods.add_function("Challenges", |lua, ()| {
            let s: AppDataRef<Sender<MainMenuButton>> = lua
                .app_data_ref()
                .ok_or(mlua::Error::external("Button app data not set"))?;
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
    service_provider: ServiceProvider,
}

impl MainMenu {
    pub fn new(service_provider: ServiceProvider) -> Self {
        let lua = LuaProvider::new_lua();
        let (tx, button_rx) = std::sync::mpsc::channel();
        lua.set_app_data(tx);
        tealr::mlu::set_global_env(ExportBindings, &lua).expect("Failed to set menu bindings");
        Self {
            lua,
            button_rx,
            control_tx: None,
            suspended: false,
            should_suspended: false,
            service_provider,
        }
    }
}

impl Scene for MainMenu {
    fn render_ui(&mut self, dt: f64) -> anyhow::Result<()> {
        let render: Function = self.lua.globals().get("render")?;
        render.call(dt / 1000.0)?;
        Ok(())
    }

    fn init(&mut self, app_control_tx: Sender<ControlMessage>) -> anyhow::Result<()> {
        self.service_provider
            .get_required::<LuaProvider>()
            .register_libraries(self.lua.clone(), "titlescreen.lua")?;
        self.control_tx = Some(app_control_tx);
        Ok(())
    }

    fn tick(&mut self, _dt: f64, _knob_state: LaserState) -> Result<()> {
        if self.should_suspended {
            self.suspended = true;
            self.should_suspended = false;
        }

        while let Ok(button) = self.button_rx.try_recv() {
            log::info!("Pressed: {:?}", &button);
            self.control_tx
                .as_ref()
                .ok_or(anyhow!("control_tx not set"))?
                .send(ControlMessage::MainMenu(button))
                .map_err(|_| anyhow!("Failed to send button"))?;
        }

        Ok(())
    }

    fn on_event(&mut self, event: &Event<UscInputEvent>) {
        if let Event::WindowEvent {
            event:
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button,
                    ..
                },
            ..
        } = event
        {
            if let Ok(mouse_pressed) = self.lua.globals().get::<_, Function>("mouse_pressed") {
                if let Err(e) = mouse_pressed.call::<_, ()>(match button {
                    winit::event::MouseButton::Left => 0,
                    winit::event::MouseButton::Right => 2,
                    winit::event::MouseButton::Middle => 1,
                    winit::event::MouseButton::Forward => 3,
                    winit::event::MouseButton::Back => 4,
                    winit::event::MouseButton::Other(b) => *b,
                }) {
                    log::error!("{}", e);
                };
            }
        }
    }

    fn on_button_pressed(
        &mut self,
        button: crate::button_codes::UscButton,
        _timestamp: SystemTime,
    ) {
        if let Ok(button_pressed) = self.lua.globals().get::<_, Function>("button_pressed") {
            if let Some(e) = button_pressed.call::<u8, ()>(button.into()).err() {
                log::error!("{:?}", e);
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

    fn debug_ui(&mut self, _ctx: &egui::Context) -> anyhow::Result<()> {
        Ok(())
    }

    fn closed(&self) -> bool {
        false
    }

    fn name(&self) -> &str {
        "Main Menu"
    }
}

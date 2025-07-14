use std::{
    rc::Rc,
    sync::mpsc::{Receiver, Sender},
    time::SystemTime,
};

use anyhow::{anyhow, Result};
use di::{RefMut, ServiceProvider};
use mlua::{self, Function, Lua};
use winit::event::{ElementState, Event, WindowEvent};

use crate::{
    button_codes::{LaserState, UscInputEvent},
    companion_interface::GameState,
    lighting::{LightingData, LightingService},
    lua_service::LuaProvider,
    scene::Scene,
    util, ControlMessage,
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

#[derive(Debug)]
struct Bindings;

#[mlua_bridge::mlua_bridge(rename_funcs = "PascalCase")]
impl Bindings {
    fn start(s: &Sender<MainMenuButton>) {
        s.send(MainMenuButton::Start).unwrap();
    }
    fn d_l_screen(s: &Sender<MainMenuButton>) {
        s.send(MainMenuButton::Downloads).unwrap();
    }
    fn multiplayer(s: &Sender<MainMenuButton>) {
        s.send(MainMenuButton::Multiplayer).unwrap();
    }
    fn exit(s: &Sender<MainMenuButton>) {
        s.send(MainMenuButton::Exit).unwrap();
    }
    fn settings(s: &Sender<MainMenuButton>) {
        s.send(MainMenuButton::Options).unwrap();
    }
    fn update(s: &Sender<MainMenuButton>) {
        s.send(MainMenuButton::Update).unwrap();
    }
    fn challenges(s: &Sender<MainMenuButton>) {
        s.send(MainMenuButton::Challenges).unwrap();
    }
}
pub struct MainMenu {
    lua: Rc<Lua>,
    button_rx: Receiver<MainMenuButton>,
    control_tx: Option<Sender<ControlMessage>>,
    should_suspended: bool,
    suspended: bool,
    service_provider: ServiceProvider,
    lighting: RefMut<LightingService>,
}

impl MainMenu {
    pub fn new(service_provider: ServiceProvider) -> Self {
        let lua = LuaProvider::new_lua();
        let (tx, button_rx) = std::sync::mpsc::channel();
        lua.set_app_data(tx);
        _ = lua.globals().set("Menu", Bindings);
        Self {
            lua,
            lighting: service_provider.get_required(),
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

    fn game_state(&self) -> crate::companion_interface::GameState {
        GameState::TitleScreen
    }

    fn tick(&mut self, _dt: f64, _knob_state: LaserState) -> Result<()> {
        if self.should_suspended {
            self.suspended = true;
            self.should_suspended = false;
        }

        let lighting = self.lighting.write().unwrap();
        let mut data = LightingData::default();
        data.buttons[util::timed_value(1000, data.buttons.len() as _) as usize] = true;
        lighting.update(data);

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
            if let Ok(mouse_pressed) = self.lua.globals().get::<Function>("mouse_pressed") {
                if let Err(e) = mouse_pressed.call::<()>(match button {
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
        if let Ok(button_pressed) = self.lua.globals().get::<Function>("button_pressed") {
            if let Some(e) = button_pressed.call::<()>(Into::<u8>::into(button)).err() {
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

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::ensure;
use di::{inject, injectable, Ref, RefMut, ServiceProvider};
use futures::executor::block_on;
use log::{info, warn};
use mlua::{Function, Lua, LuaSerdeExt, RegistryKey};
use multiplayer_protocol::messages::client::ClientCommand;
use multiplayer_protocol::messages::types::GameScore;
use multiplayer_protocol::messages::{self, get_topic};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::RwLock;
use tokio::{net::ToSocketAddrs, task::JoinHandle};

use crate::async_service::AsyncService;
use crate::config::GameConfig;
use crate::lua_service::LuaProvider;
use crate::scene::{Scene, SceneData};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MultiplayerState {
    Disconnected,
    Connected,
}

pub struct MultiplayerService {
    state: MultiplayerState,
    reader_task: JoinHandle<()>,
    writer_task: JoinHandle<()>,
    tx: Sender<multiplayer_protocol::messages::server::ServerCommand>,
    rx: Receiver<multiplayer_protocol::messages::client::ClientCommand>,
    user_id: Arc<RwLock<String>>,
}

#[injectable]
impl MultiplayerService {
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        ensure!(
            !GameConfig::get().multiplayer.server.is_empty(),
            "No multiplayer server set"
        );
        if !matches!(self.state, MultiplayerState::Disconnected) {
            self.reader_task.abort();
            self.writer_task.abort();
        }

        let (mut cmd_rx, mut cmd_tx) =
            multiplayer_protocol::connect(&GameConfig::get().multiplayer.server).await?;

        let (tx, rx) = channel(100);
        let user_id = self.user_id.clone();
        self.reader_task = tokio::spawn(async move {
            while let Ok(msg) = cmd_rx.read().await.inspect_err(|e| warn!("{e}")) {
                if let ClientCommand::Info(i) = &msg {
                    let mut id = user_id.write().await;
                    *id = i.userid.clone();
                }
                tx.send(msg).await.unwrap();
            }
        });
        self.rx = rx;

        let (tx, mut rx) = channel(100);
        self.writer_task = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                info!("Sending: {:?}", &msg);
                cmd_tx.write(&msg).await.unwrap();
            }
        });

        self.tx = tx;
        self.state = MultiplayerState::Connected;

        Ok(())
    }

    pub fn disconnect(&mut self) {
        self.writer_task.abort();
        self.reader_task.abort();
        self.state = MultiplayerState::Disconnected;
    }

    pub fn state(&self) -> MultiplayerState {
        self.state
    }

    pub fn poll(&mut self) -> Option<messages::client::ClientCommand> {
        self.rx.try_recv().ok()
    }

    pub fn send(&mut self, cmd: messages::server::ServerCommand) -> anyhow::Result<()> {
        Ok(self.tx.try_send(cmd)?)
    }

    pub fn user_id(&self) -> String {
        self.user_id.blocking_read().clone()
    }

    #[inject]
    pub fn new() -> Self {
        let dummy_r = tokio::spawn(async move {});
        let dummy_w = tokio::spawn(async move {});
        let (_, rx) = channel(1);
        let (tx, _) = channel(1);
        Self {
            state: MultiplayerState::Disconnected,
            reader_task: dummy_r,
            writer_task: dummy_w,
            tx,
            rx,
            user_id: Arc::new(RwLock::const_new(String::new())),
        }
    }
}

impl SceneData for RefMut<MultiplayerService> {
    fn make_scene(
        self: Box<Self>,
        service_provider: ServiceProvider,
    ) -> anyhow::Result<Box<dyn Scene>> {
        {
            ensure!(
                self.read().unwrap().state() == MultiplayerState::Connected,
                "Not connected"
            )
        }
        Ok(Box::new(MultiplayerScreen::new(
            service_provider,
            *self.clone(),
        )?))
    }
}

pub struct MultiplayerScreen {
    sp: ServiceProvider,
    service: RefMut<MultiplayerService>,
    lua: Rc<Lua>,
    should_suspend: bool,
    suspended: bool,
    closing: bool,
    topic_handlers: HashMap<String, RegistryKey>,
}

impl MultiplayerScreen {
    pub fn new(sp: ServiceProvider, service: RefMut<MultiplayerService>) -> anyhow::Result<Self> {
        let lua_service: Ref<LuaProvider> = sp.get_required();
        let lua = LuaProvider::new_lua();
        lua_service.register_libraries(lua.clone(), "multiplayerscreen.lua")?;
        let topic_handlers: HashMap<String, RegistryKey> = HashMap::new();
        lua.set_app_data(topic_handlers);
        lua.globals().set("Tcp", LuaTcp)?;
        if let Ok(init_func) = lua.globals().get::<Function>("init_tcp") {
            init_func.call::<()>(())?;
        }

        let topic_handlers = lua.remove_app_data().unwrap();

        Ok(MultiplayerScreen {
            service,
            lua,
            sp,
            should_suspend: false,
            suspended: false,
            closing: false,
            topic_handlers,
        })
    }
}

const MULTI_VERSION: &'static str = "v0.19";

impl Scene for MultiplayerScreen {
    fn render_ui(&mut self, dt: f64) -> anyhow::Result<()> {
        let f: Function = self.lua.globals().get("render")?;
        f.call::<()>(dt / 1000.0)?;
        Ok(())
    }

    fn init(
        &mut self,
        app_control_tx: std::sync::mpsc::Sender<crate::ControlMessage>,
    ) -> anyhow::Result<()> {
        let mut s = self.service.write().unwrap();
        s.send(messages::server::ServerCommand::Auth(
            messages::server::Auth {
                password: String::new(),
                name: GameConfig::get().multiplayer.name.clone(),
                version: MULTI_VERSION.into(),
            },
        ))?;

        Ok(())
    }

    fn tick(&mut self, dt: f64, knob_state: crate::button_codes::LaserState) -> anyhow::Result<()> {
        let mut s = self.service.write().unwrap();
        while let Some(msg) = s.poll() {
            info!("Recieved {:#?}", &msg);

            self.lua.globals().set("screenState", "roomList")?;

            match &msg {
                ClientCommand::Info(..) => s.send(messages::server::ServerCommand::Rooms)?,
                _ => {}
            }

            let Some(topic) = get_topic(&msg) else {
                break;
            };

            let Some(handler) = self.topic_handlers.get(&topic) else {
                break;
            };

            let Ok(handler) = self.lua.registry_value::<Function>(handler) else {
                break;
            };

            handler.call::<()>(self.lua.to_value(&msg))?;
        }
        Ok(())
    }

    fn suspend(&mut self) {
        self.should_suspend = true;
    }

    fn is_suspended(&self) -> bool {
        self.suspended
    }

    fn resume(&mut self) {
        self.suspended = false;
    }

    fn debug_ui(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        Ok(())
    }

    fn closed(&self) -> bool {
        self.closing
    }

    fn name(&self) -> &str {
        "Multiplayer screen"
    }

    fn on_button_pressed(
        &mut self,
        button: crate::button_codes::UscButton,
        timestamp: std::time::SystemTime,
    ) {
        match button {
            crate::button_codes::UscButton::Back => self.closing = true,
            _ => {}
        }
    }
}

pub struct LuaTcp;

#[mlua_bridge::mlua_bridge(rename_funcs = "PascalCase")]
impl LuaTcp {
    fn set_topic_handler(
        handlers: &mut HashMap<String, RegistryKey>,
        lua: &Lua,
        topic: String,
        callback: Function,
    ) -> Result<(), mlua::Error> {
        info!("Topic handler set for: {}", &topic);
        let function_key = lua.create_registry_value(callback)?;
        handlers.insert(topic, function_key);
        Ok(())
    }
}

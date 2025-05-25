use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{ensure, Context};
use di::{inject, injectable, Ref, RefMut, ServiceProvider};
use futures::executor::block_on;
use log::{info, warn};
use mlua::{Function, Lua, LuaSerdeExt, RegistryKey, UserData};
use multiplayer_protocol::messages::client::ClientCommand;
use multiplayer_protocol::messages::server::New;
use multiplayer_protocol::messages::types::GameScore;
use multiplayer_protocol::messages::{self, get_topic};
use serde::Serialize;
use serde_json::json;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::RwLock;
use tokio::{net::ToSocketAddrs, task::JoinHandle};
use winit::event::{ElementState, Event};

use crate::async_service::AsyncService;
use crate::config::GameConfig;
use crate::input_state::InputState;
use crate::lua_service::LuaProvider;
use crate::scene::{Scene, SceneData};
use crate::settings_dialog::SettingsDialog;
use crate::song_provider::{LoadSongFn, SongDiffId, SongProvider};
use crate::songselect::Song;
use crate::util::laser_navigation::LaserNavigation;
use crate::util::Warn;
use crate::{util, ControlMessage, FileSongProvider, NauticaSongProvider};

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
    rate: Arc<AtomicI32>,
}

#[derive(Debug, Clone, Serialize)]
struct MpScreenDiff {
    difficulty: u8,
    level: u8,
}

#[derive(Debug, Clone, Serialize, Default)]
struct MpScreenSong {
    self_picked: bool,
    jacket: u32,
    title: String,
    artist: String,
    #[serde(rename = "jacketPath")]
    jacket_path: String,
    diff_index: u8,
    all_difficulties: Vec<MpScreenDiff>,
    min_bpm: u32,
    max_bpm: u32,
    speed_bpm: u32,
    hispeed: f32,
}

impl From<&Song> for MpScreenSong {
    fn from(value: &Song) -> Self {
        let diffs = value.difficulties.read().unwrap();

        Self {
            self_picked: false,
            jacket: 0,
            title: value.title.clone(),
            artist: value.artist.clone(),
            jacket_path: diffs.first().unwrap().jacket_path.display().to_string(),
            diff_index: 0,
            all_difficulties: diffs
                .iter()
                .map(|x| MpScreenDiff {
                    difficulty: x.difficulty,
                    level: x.level,
                })
                .collect(),
            ..Default::default()
        }
    }
}

#[injectable]
impl MultiplayerService {
    pub fn rate(&self) -> i32 {
        self.rate.load(std::sync::atomic::Ordering::Relaxed)
    }

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
        let rate = self.rate.clone();

        self.reader_task = tokio::spawn(async move {
            loop {
                while let Ok(msg) = cmd_rx
                    .read()
                    .await
                    .inspect_err(|e| warn!("Recieve error: {e}"))
                {
                    if let ClientCommand::Info(i) = &msg {
                        let mut id = user_id.write().await;
                        *id = i.userid.clone();
                        rate.store(i.refresh_rate, std::sync::atomic::Ordering::Relaxed);
                    }
                    tx.send(msg).await.unwrap();
                }
                tokio::time::sleep(Duration::from_millis(100)).await
            }
        });
        self.rx = rx;

        let (tx, mut rx) = channel(100);
        self.writer_task = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                info!("Sending: {:?}", &msg);
                cmd_tx
                    .write(&msg)
                    .await
                    .inspect_err(|e| warn!("Multiplayer communication error: {e}"))
                    .unwrap();
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
            rate: Arc::new(AtomicI32::new(1000)),
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
    state: MultiplayerScreenState,
    input_state: InputState,
    lua_commands: std::sync::mpsc::Receiver<MpScreenCommand>,
    poll_interval: u16,
    program_control: std::sync::mpsc::Sender<ControlMessage>,
    chart_hash: String,
    song: Option<Arc<Song>>,
    laser_nav: LaserNavigation,
    selected_index: usize,
    host: bool,
    song_dialog: SettingsDialog,
    close_dialog: Arc<AtomicBool>,
}

impl MultiplayerScreen {
    pub fn new(sp: ServiceProvider, service: RefMut<MultiplayerService>) -> anyhow::Result<Self> {
        let lua_service: Ref<LuaProvider> = sp.get_required();
        let lua = LuaProvider::new_lua();
        lua_service.register_libraries(lua.clone(), "multiplayerscreen.lua")?;
        lua.globals().set("mpScreen", MpScreen)?;

        let state = if GameConfig::get().multiplayer.name.is_empty() {
            MultiplayerScreenState::SetUsername(String::new())
        } else {
            MultiplayerScreenState::RoomList
        };

        let (tx, rx) = std::sync::mpsc::channel();
        lua.set_app_data(tx);
        lua.globals().set("screenState", state.as_str())?;

        let topic_handlers = LuaTcp::init(&lua, service.clone());
        let is = InputState::clone(&sp.get_required());

        let close_dialog = Arc::new(AtomicBool::new(false));
        Ok(MultiplayerScreen {
            input_state: is.clone(),
            service,
            lua,
            sp,
            should_suspend: false,
            suspended: false,
            closing: false,
            topic_handlers,
            state,
            lua_commands: rx,
            poll_interval: 32,
            program_control: std::sync::mpsc::channel().0,
            chart_hash: String::new(),
            song: None,
            laser_nav: LaserNavigation::new(),
            selected_index: 0,
            host: false,
            close_dialog: close_dialog.clone(),
            song_dialog: SettingsDialog::new_empty(),
        })
    }

    fn set_text_input(&self) {
        let text = match &self.state {
            MultiplayerScreenState::SetUsername(t) => t,
            MultiplayerScreenState::PasswordScreen(t, _room_id) => t,
            MultiplayerScreenState::NewRoomName(t) => t,
            MultiplayerScreenState::NewRoomPassword(t, _room_name) => t,
            _ => {
                self.input_state.set_text_input_active(false);
                return;
            }
        };

        self.input_state.set_text_input_active(true);
        let v = self
            .lua
            .to_value(&json!({"text": text}))
            .expect("Could not make lua value");

        self.lua
            .globals()
            .set("textInput", v)
            .expect("Coult not set lua value")
    }

    fn auth(&self) -> anyhow::Result<()> {
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

    fn submit(&mut self) {
        match &self.state {
            MultiplayerScreenState::SetUsername(name) => {
                GameConfig::get_mut().multiplayer.name = name.clone();
                _ = self.auth();
                self.state = MultiplayerScreenState::RoomList;
            }
            MultiplayerScreenState::RoomList => {}
            MultiplayerScreenState::PasswordScreen(password, id) => {
                let mut s = self.service.write().unwrap();
                _ = s.send(messages::server::ServerCommand::RoomJoin(
                    messages::server::Join {
                        id: Some(id.clone()),
                        password: Some(password.clone()),
                        token: None,
                    },
                ));
            }
            MultiplayerScreenState::NewRoomName(_) => {}
            MultiplayerScreenState::NewRoomPassword(..) => {}
            MultiplayerScreenState::InRoom => {}
        }
    }

    fn process_server_commands(&mut self) -> anyhow::Result<()> {
        let mut s = self.service.write().unwrap();

        while let Some(msg) = s.poll() {
            info!("Recieved {:#?}", &msg);

            match (&msg, self.state.discriminant()) {
                (ClientCommand::Info(..), _) => s.send(messages::server::ServerCommand::Rooms)?,
                (ClientCommand::Rooms(..), _) => {
                    self.state = MultiplayerScreenState::RoomList;
                }
                (ClientCommand::Joined(_), _) => {
                    self.state = MultiplayerScreenState::InRoom;
                }
                (ClientCommand::RoomUpdate(u), _) => {
                    self.host = u.host == s.user_id();

                    if u.chart_hash.as_ref().is_some_and(|x| *x != self.chart_hash) {
                        self.chart_hash = u.chart_hash.clone().unwrap_or_default();
                        let host: bool = self.host;
                        // TODO: Search for song in a task
                        let song_diff = self.set_song(u, host).ok();
                        if let Some((song, diff)) = song_diff {
                            self.song = Some(song);
                            self.selected_index = diff;
                            s.send(messages::server::ServerCommand::Level(
                                messages::server::Level {
                                    level: u.level.unwrap_or_default(),
                                },
                            ))?;
                        } else {
                            self.selected_index = 0;
                            self.song = None;
                            _ = s.send(messages::server::ServerCommand::Nomap {});
                        }
                    }
                }
                (ClientCommand::Started(..), _) => {
                    let song = self.song.clone().context("Song not found")?;
                    let loader = self.load_song(&song)?;
                    _ = self.program_control.send(ControlMessage::Song {
                        song,
                        diff: 0,
                        loader: loader,
                        autoplay: crate::game_main::AutoPlay::None,
                    });
                }

                _ => {}
            }

            let Some(topic) = get_topic(&msg) else {
                continue;
            };

            let Some(handler) = self.topic_handlers.get(&topic) else {
                continue;
            };

            let Ok(handler) = self.lua.registry_value::<Function>(handler) else {
                continue;
            };

            handler.call::<()>(self.lua.to_value_with(
                &msg,
                mlua::SerializeOptions::new().serialize_none_to_null(false),
            ))?;
        }

        Ok(())
    }

    fn process_lua_commands(&mut self) -> anyhow::Result<()> {
        while let Ok(cmd) = self.lua_commands.try_recv() {
            info!("Clicked {}", cmd.as_ref());
            match (cmd, &mut self.state) {
                (MpScreenCommand::OpenSettings, _) => {}
                (MpScreenCommand::SelectSong, _) => {
                    self.song_dialog.show = true;
                }
                (
                    MpScreenCommand::BeginJoinWithPassword(room_id),
                    MultiplayerScreenState::RoomList,
                ) => {
                    self.state = MultiplayerScreenState::PasswordScreen(String::new(), room_id);
                }
                (
                    MpScreenCommand::JoinWithPassword,
                    MultiplayerScreenState::PasswordScreen(password, room_id),
                ) => {
                    let mut s = self.service.write().unwrap();
                    s.send(messages::server::ServerCommand::RoomJoin(
                        messages::server::Join {
                            id: Some(room_id.clone()),
                            password: Some(password.clone()),
                            token: None,
                        },
                    ))?;
                }
                (
                    MpScreenCommand::JoinWithoutPassword(room_id),
                    MultiplayerScreenState::RoomList,
                ) => {
                    let mut s = self.service.write().unwrap();
                    s.send(messages::server::ServerCommand::RoomJoin(
                        messages::server::Join {
                            id: Some(room_id),
                            password: None,
                            token: None,
                        },
                    ))?;
                }
                (MpScreenCommand::NewRoomStep, MultiplayerScreenState::NewRoomName(name)) => {
                    self.state = MultiplayerScreenState::NewRoomPassword(
                        String::new(),
                        std::mem::take(name),
                    );
                }
                (
                    MpScreenCommand::NewRoomStep,
                    MultiplayerScreenState::NewRoomPassword(password, name),
                ) => {
                    let mut s = self.service.write().unwrap();
                    let name = std::mem::take(name);
                    let password = (!password.is_empty()).then(|| std::mem::take(password));
                    s.send(messages::server::ServerCommand::RoomNew(New {
                        name,
                        password,
                    }))?;
                    self.state = MultiplayerScreenState::RoomList
                }
                (MpScreenCommand::NewRoomStep, _) => {
                    self.state = MultiplayerScreenState::NewRoomName(String::new());
                }

                (MpScreenCommand::SaveUsername, _) => {
                    if let MultiplayerScreenState::SetUsername(name) = &self.state {
                        if !name.is_empty() {
                            {
                                GameConfig::get_mut().multiplayer.name = name.clone();
                            }
                            self.auth()?;
                        }
                    }
                }
                (MpScreenCommand::Exit, _) => self.closing = true,
                (_, _) => {}
            }
        }
        Ok(())
    }

    fn advance_selection(&mut self) {
        //Poll y to clear state
        self.laser_nav.poll_y();
        let selection = self.laser_nav.poll_x();
        let Some(song) = self.song.as_ref() else {
            return;
        };

        if selection == 0 {
            return;
        }

        let mut service = self.service.write().unwrap();

        let diffs = song.difficulties.read().unwrap();
        let diff_count = diffs.len();

        if diff_count == 0 {
            self.selected_index = 0;
            return;
        }

        self.selected_index = self
            .selected_index
            .saturating_add_signed(selection as isize)
            .min(diff_count.saturating_sub(1));

        let diff = &diffs[self.selected_index];

        _ = service.send(messages::server::ServerCommand::Level(
            messages::server::Level {
                level: diff.level as _,
            },
        ));

        _ = self.set_lua_song(song.as_ref(), self.host, self.selected_index);
    }

    fn set_lua_song(&self, song: &Song, self_picked: bool, diff: usize) -> anyhow::Result<()> {
        let mut lua_song = MpScreenSong::from(song);

        lua_song.self_picked = self_picked;
        lua_song.diff_index = diff as _;

        self.lua
            .globals()
            .set("selected_song", self.lua.to_value(&lua_song)?)?;

        Ok(())
    }

    fn set_song(
        &self,
        u: &messages::client::Update,
        self_picked: bool,
    ) -> anyhow::Result<(Arc<Song>, usize)> {
        let nautica = self.sp.get_required_mut::<NauticaSongProvider>();
        let files = self.sp.get_required_mut::<FileSongProvider>();
        let files = files.write().ok().context("Lock fail")?;
        let hash = u.chart_hash.as_ref().context("No hash")?.as_str();
        let song = u.song.as_ref().context("No song")?.as_str();
        let diff = u.diff.unwrap_or_default();
        let level = u.level.unwrap_or_default();

        let song = nautica
            .write()
            .ok()
            .context("Lock fail")?
            .get_multiplayer_song(hash, song, diff, level)
            .or_else(|_| files.get_multiplayer_song(hash, song, diff, level))?;

        let diff_index = song
            .difficulties
            .read()
            .unwrap()
            .iter()
            .enumerate()
            .find(|x| {
                x.1.difficulty == u.diff.unwrap_or_default() as u8
                    && x.1.level == u.level.unwrap_or_default() as u8
            })
            .map(|x| x.0)
            .unwrap_or_default();

        self.set_lua_song(song.as_ref(), self_picked, diff_index);

        Ok((song, diff_index))
    }

    fn load_song(&self, song: &Song) -> anyhow::Result<LoadSongFn> {
        let diffs = song.difficulties.read().unwrap();
        let diff = diffs.get(self.selected_index).context("No diffs")?;
        let song_diff_id = SongDiffId::SongDiff(song.id.clone(), diff.id.clone());
        //TODO: Better song provider handling
        match &song.id {
            crate::SongId::Missing => unreachable!(),
            crate::SongId::IntId(_) => {
                let sp = self.sp.get_required_mut::<FileSongProvider>();
                let sp = sp.write().unwrap();
                sp.load_song(&song_diff_id)
            }
            crate::SongId::StringId(_) => {
                let sp = self.sp.get_required_mut::<NauticaSongProvider>();
                let sp = sp.write().unwrap();
                sp.load_song(&song_diff_id)
            }
        }
    }
}

const MULTI_VERSION: &'static str = "v0.19";

impl Scene for MultiplayerScreen {
    fn render_ui(&mut self, dt: f64) -> anyhow::Result<()> {
        if self.suspended {
            return Ok(());
        }

        let f: Function = self.lua.globals().get("render")?;
        f.call::<()>(dt / 1000.0)?;
        self.song_dialog.render(dt)?;
        Ok(())
    }

    fn init(
        &mut self,
        app_control_tx: std::sync::mpsc::Sender<crate::ControlMessage>,
    ) -> anyhow::Result<()> {
        if !GameConfig::get().multiplayer.name.is_empty() {
            self.auth()?;
        }
        self.song_dialog = SettingsDialog::song_provider_select(
            self.input_state.clone(),
            self.sp.create_scope(),
            self.close_dialog.clone(),
            app_control_tx.clone(),
        );
        let lua_service: Ref<LuaProvider> = self.sp.get_required();
        self.song_dialog.init_lua(&lua_service)?;
        self.set_text_input();
        self.program_control = app_control_tx;
        Ok(())
    }

    fn tick(
        &mut self,
        _dt: f64,
        _knob_state: crate::button_codes::LaserState,
    ) -> anyhow::Result<()> {
        if self.close_dialog.load(std::sync::atomic::Ordering::Relaxed) {
            self.song_dialog.show = false;
            self.close_dialog
                .store(false, std::sync::atomic::Ordering::Relaxed);
        }
        if self.should_suspend {
            self.suspended = true;
            self.should_suspend = false;
            return Ok(());
        }

        if !self.suspended {
            let begin_state = self.state.discriminant();
            self.process_server_commands()?;
            self.process_lua_commands()?;
            self.advance_selection();

            if begin_state != self.state.discriminant() {
                self.lua.globals().set("screenState", self.state.as_str())?;
                self.set_text_input();
            }
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
        let mut s = self.service.write().unwrap();
        _ = s.send(messages::server::ServerCommand::GetUpdate);

        self.suspended = false;
    }

    fn debug_ui(&mut self, _ctx: &egui::Context) -> anyhow::Result<()> {
        Ok(())
    }

    fn closed(&self) -> bool {
        self.closing
    }

    fn name(&self) -> &str {
        "Multiplayer screen"
    }

    fn on_event(&mut self, event: &winit::event::Event<crate::button_codes::UscInputEvent>) {
        if self.song_dialog.show {
            if let Event::UserEvent(e) = event {
                self.song_dialog.on_input(e);
            }
            return;
        }

        match event {
            winit::event::Event::WindowEvent { event, .. } => match event {
                winit::event::WindowEvent::KeyboardInput { event, .. } => {
                    let submit = event.logical_key
                        == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Enter)
                        && event.state == winit::event::ElementState::Pressed;
                    if submit {
                        self.submit();
                    }
                }
                winit::event::WindowEvent::MouseInput { state, .. }
                    if *state == ElementState::Pressed =>
                {
                    _ = self
                        .lua
                        .globals()
                        .get::<Function>("mouse_pressed")
                        .and_then(|x| x.call::<u8>(0))
                        .inspect_err(|e| warn!("{e}"));
                }
                _ => {}
            },
            winit::event::Event::UserEvent(e) => match e {
                crate::button_codes::UscInputEvent::Laser(laser_state, ..) => {
                    self.laser_nav.update(*laser_state)
                }
                _ => {}
            },
            _ => {}
        }

        if self.state.text_state() {
            let text = match &mut self.state {
                MultiplayerScreenState::SetUsername(t) => t,
                MultiplayerScreenState::PasswordScreen(t, _room_id) => t,
                MultiplayerScreenState::NewRoomName(t) => t,
                MultiplayerScreenState::NewRoomPassword(t, _room_name) => t,
                MultiplayerScreenState::RoomList => unreachable!(),
                MultiplayerScreenState::InRoom => unreachable!(),
            };

            if util::do_text_event(text, event) {
                self.set_text_input();
            }
        }
    }

    fn on_button_pressed(
        &mut self,
        button: crate::button_codes::UscButton,
        _timestamp: std::time::SystemTime,
    ) {
        if self.song_dialog.show {
            self.song_dialog.on_button_press(button);
            return;
        }

        match button {
            crate::button_codes::UscButton::Back => self.closing = true,
            _ => {}
        }
    }
}

impl Drop for MultiplayerScreen {
    fn drop(&mut self) {
        if let Ok(mut s) = self.service.write() {
            _ = s.send(messages::server::ServerCommand::Leave);
        }
    }
}

pub struct LuaTcp;

impl LuaTcp {
    pub fn init(lua: &Lua, service: RefMut<MultiplayerService>) -> HashMap<String, RegistryKey> {
        let topic_handlers: HashMap<String, RegistryKey> = HashMap::new();
        if let Err(e) = lua.globals().set("Tcp", LuaTcp) {
            log::error!("{e}");
            return topic_handlers;
        }
        lua.set_app_data(topic_handlers);
        lua.set_app_data(service);
        if let Ok(init_func) = lua.globals().get::<Function>("init_tcp") {
            init_func.call::<()>(()).warn("Lua TCP init error");
        }
        let topic_handlers = lua.remove_app_data().unwrap();
        topic_handlers
    }
}

#[mlua_bridge::mlua_bridge(rename_funcs = "PascalCase")]
impl LuaTcp {
    fn send_line(s: &RefMut<MultiplayerService>, msg: String) -> Result<(), mlua::Error> {
        let mut service = s.write().unwrap();
        service
            .send(messages::server::ServerCommand::Raw(msg))
            .map_err(mlua::Error::external)
    }

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

use strum::{EnumDiscriminants, IntoStaticStr};
#[derive(EnumDiscriminants, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "camelCase")]
enum MultiplayerScreenState {
    SetUsername(String),
    RoomList,
    PasswordScreen(String, String),
    NewRoomName(String),
    /// (password, name)
    NewRoomPassword(String, String),
    InRoom,
}

impl MultiplayerScreenState {
    fn as_str(&self) -> &'static str {
        self.into()
    }

    fn text_state(&self) -> bool {
        match self {
            MultiplayerScreenState::SetUsername(_) => true,
            MultiplayerScreenState::RoomList => false,
            MultiplayerScreenState::PasswordScreen(..) => true,
            MultiplayerScreenState::NewRoomName(_) => true,
            MultiplayerScreenState::NewRoomPassword(..) => true,
            MultiplayerScreenState::InRoom => false,
        }
    }

    fn discriminant(&self) -> MultiplayerScreenStateDiscriminants {
        self.into()
    }
}

struct MpScreen;

#[derive(Clone, Debug, strum::AsRefStr, strum::EnumIter)]
enum MpScreenCommand {
    OpenSettings,
    SelectSong,
    BeginJoinWithPassword(String),
    JoinWithPassword,
    JoinWithoutPassword(String),
    NewRoomStep,
    SaveUsername,
    Exit,
}

#[mlua_bridge::mlua_bridge(rename_funcs = "PascalCase")]
impl MpScreen {
    fn open_settings(tx: &mut std::sync::mpsc::Sender<MpScreenCommand>) {
        tx.send(MpScreenCommand::OpenSettings)
            .warn("Multi screen dropped recieveer");
    }
    fn select_song(tx: &mut std::sync::mpsc::Sender<MpScreenCommand>) {
        tx.send(MpScreenCommand::SelectSong)
            .warn("Multi screen dropped recieveer");
    }
    fn join_with_password(
        tx: &mut std::sync::mpsc::Sender<MpScreenCommand>,
        room_id: Option<String>,
    ) {
        match room_id {
            Some(room_id) => tx.send(MpScreenCommand::BeginJoinWithPassword(room_id)),
            None => tx.send(MpScreenCommand::JoinWithPassword),
        }
        .warn("Multi screen dropped recieveer");
    }
    fn join_without_password(tx: &mut std::sync::mpsc::Sender<MpScreenCommand>, room_id: String) {
        tx.send(MpScreenCommand::JoinWithoutPassword(room_id))
            .warn("Multi screen dropped recieveer");
    }
    fn new_room_step(tx: &mut std::sync::mpsc::Sender<MpScreenCommand>) {
        tx.send(MpScreenCommand::NewRoomStep)
            .warn("Multi screen dropped recieveer");
    }
    fn save_username(tx: &mut std::sync::mpsc::Sender<MpScreenCommand>) {
        tx.send(MpScreenCommand::SaveUsername)
            .warn("Multi screen dropped recieveer");
    }
    fn exit(tx: &mut std::sync::mpsc::Sender<MpScreenCommand>) {
        tx.send(MpScreenCommand::Exit)
            .warn("Multi screen dropped recieveer");
    }
}

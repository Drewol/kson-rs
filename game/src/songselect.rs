use anyhow::{ensure, Result};
use di::{RefMut, ServiceProvider};
use game_loop::winit::event::{
    ElementState, Event, Ime, KeyboardInput, VirtualKeyCode, WindowEvent,
};
use itertools::Itertools;
use log::warn;
use puffin::{profile_function, profile_scope};
use rodio::Source;
use serde::Serialize;
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize},
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex, RwLock,
    },
    time::{Duration, SystemTime},
};
use tealr::{
    mlu::{
        mlua::{Function, Lua, LuaSerdeExt},
        TealData, UserData,
    },
    SingleType, ToTypename,
};

use crate::{
    button_codes::{LaserAxis, LaserState, UscButton, UscInputEvent},
    input_state::InputState,
    lua_service::LuaProvider,
    results::Score,
    scene::{Scene, SceneData},
    settings_dialog::SettingsDialog,
    song_provider::{
        DiffId, ScoreProvider, ScoreProviderEvent, SongDiffId, SongId, SongProvider,
        SongProviderEvent,
    },
    sources::owned_source::owned_source,
    take_duration_fade::take_duration_fade,
    ControlMessage, RuscMixer,
};

mod song_collection;
use song_collection::*;

#[derive(Debug, ToTypename, Clone, Serialize, UserData)]
#[serde(rename_all = "camelCase")]
pub struct Difficulty {
    pub jacket_path: PathBuf,
    pub level: u8,
    pub difficulty: u8, // 0 = nov, 1 = adv, etc.
    pub id: DiffId,     //unique static identifier
    pub effector: String,
    pub top_badge: i32,     //top badge for this difficulty
    pub scores: Vec<Score>, //array of all scores on this diff
    pub hash: Option<String>,
}

impl TealData for Difficulty {
    fn add_fields<'lua, F: tealr::mlu::TealDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("jacketPath", |_, diff| {
            Ok(diff
                .jacket_path
                .clone()
                .into_os_string()
                .into_string()
                .unwrap())
        });
        fields.add_field_method_get("level", |_, diff| Ok(diff.level));
        fields.add_field_method_get("difficulty", |_, diff| Ok(diff.difficulty));
        fields.add_field_method_get("id", |_, diff| Ok(diff.id.clone()));
        fields.add_field_method_get("effector", |_, diff| Ok(diff.effector.clone()));
        fields.add_field_method_get("topBadge", |_, diff| Ok(diff.top_badge));
        fields.add_field_method_get("scores", |_, diff| Ok(diff.scores.clone()));
    }
}

#[derive(Debug, ToTypename, UserData, Clone, Serialize, Default)]
pub struct Song {
    pub title: String,
    pub artist: String,
    pub bpm: String,                                //ex. "170-200"
    pub id: SongId,                                 //unique static identifier
    pub difficulties: Arc<RwLock<Vec<Difficulty>>>, //array of all difficulties for this song
}

//Keep tealdata for generating type definitions
impl TealData for Song {
    fn add_fields<'lua, F: tealr::mlu::TealDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("title", |_, song| Ok(song.title.clone()));
        fields.add_field_method_get("artist", |_, song| Ok(song.artist.clone()));
        fields.add_field_method_get("bpm", |_, song| Ok(song.bpm.clone()));
        fields.add_field_method_get("id", |_, song| Ok(song.id.clone()));
        fields.add_field_method_get("difficulties", |_, song| {
            Ok(song.difficulties.read().unwrap().clone())
        });
    }
}

#[derive(Serialize, UserData)]
#[serde(rename_all = "camelCase")]
pub struct SongSelect {
    songs: SongCollection,
    search_input_active: bool, //true when the user is currently inputting search text
    search_text: String,       //current string used by the song search
    selected_index: i32,
    selected_diff_index: i32,
    preview_countdown: f64,
    preview_finished: Arc<AtomicUsize>,
    preview_playing: Arc<AtomicU64>,
}

impl TealData for SongSelect {
    fn add_fields<'lua, F: tealr::mlu::TealDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("songs", |_, _| Ok([] as [Song; 0]));
        fields.add_field_method_get("searchInputActive", |_, songwheel| {
            Ok(songwheel.search_input_active)
        });
        fields.add_field_method_get("searchText", |_, songwheel| {
            Ok(songwheel.search_text.clone())
        });
        fields.add_field_method_get(
            "searchStatus",
            |_, _| -> Result<Option<String>, tealr::mlu::mlua::Error> { Ok(None) },
        );
    }
}

impl ToTypename for SongSelect {
    fn to_typename() -> tealr::Type {
        tealr::Type::Single(SingleType {
            name: tealr::Name(std::borrow::Cow::Borrowed("songwheel")),
            kind: tealr::KindOfType::External,
        })
    }
}

type SyncSongProvider = Arc<Mutex<dyn SongProvider>>;
type SyncScoreProvider = Arc<Mutex<dyn ScoreProvider>>;

impl SongSelect {
    pub fn new() -> Self {
        Self {
            songs: Default::default(),
            search_input_active: false,
            search_text: String::new(),
            selected_index: 0,
            selected_diff_index: 0,
            preview_countdown: 1500.0,
            preview_finished: Arc::new(AtomicUsize::new(0)),
            preview_playing: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl SceneData for SongSelect {
    fn make_scene(
        self: Box<Self>,
        service_provider: ServiceProvider,
    ) -> anyhow::Result<Box<dyn Scene>> {
        Ok(Box::new(SongSelectScene::new(self, service_provider)))
    }
}
pub const KNOB_NAV_THRESHOLD: f32 = std::f32::consts::PI / 3.0;

pub struct SongSelectScene {
    state: Box<SongSelect>,
    lua: Rc<Lua>,
    background_lua: Rc<Lua>,
    program_control: Option<Sender<ControlMessage>>,
    song_advance: f32,
    diff_advance: f32,
    suspended: Arc<AtomicBool>,
    closed: bool,
    mixer: RuscMixer,
    _sample_owner: Receiver<()>,
    sample_marker: Sender<()>,
    settings_dialog: SettingsDialog,
    input_state: InputState,
    services: ServiceProvider,
    song_provider: RefMut<dyn SongProvider>,
    song_events: bus::BusReader<SongProviderEvent>,
    score_events: bus::BusReader<ScoreProviderEvent>,
    score_provider: RefMut<dyn ScoreProvider>, //TODO
}

impl SongSelectScene {
    pub fn new(mut song_select: Box<SongSelect>, services: ServiceProvider) -> Self {
        let (sample_marker, sample_owner) = channel();
        let input_state = InputState::clone(&services.get_required());
        let song_provider: RefMut<dyn SongProvider> = services.get_required();
        let score_provider: RefMut<dyn ScoreProvider> = services.get_required();
        let score_events = score_provider.write().unwrap().subscribe();
        let song_events = song_provider.write().unwrap().subscribe();
        let initial_songs = song_provider.write().unwrap().get_all();
        _ = score_provider
            .write()
            .unwrap()
            .init_scores(&mut initial_songs.iter());
        song_select.songs.append(initial_songs);
        Self {
            background_lua: Rc::new(Lua::new()),
            lua: Rc::new(Lua::new()),
            state: song_select,
            program_control: None,
            diff_advance: 0.0,
            song_advance: 0.0,
            suspended: Arc::new(AtomicBool::new(false)),
            closed: false,
            mixer: services.get_required(),
            sample_marker,
            _sample_owner: sample_owner,
            input_state: input_state.clone(),
            settings_dialog: SettingsDialog::general_settings(input_state),
            song_events,
            score_events,
            song_provider,
            score_provider,
            services,
        }
    }

    fn update_lua(&self) -> anyhow::Result<()> {
        Ok(self
            .lua
            .globals()
            .set("songwheel", self.lua.to_value(&self.state)?)?)
    }
}

impl Scene for SongSelectScene {
    fn render_ui(&mut self, dt: f64) -> Result<()> {
        profile_function!();
        let render_bg: Function = self.background_lua.globals().get("render")?;
        render_bg.call(dt / 1000.0)?;

        let render_wheel: Function = self.lua.globals().get("render")?;
        render_wheel.call(dt / 1000.0)?;

        self.settings_dialog.render(dt)?;

        Ok(())
    }

    fn is_suspended(&self) -> bool {
        self.suspended.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn debug_ui(&mut self, ctx: &egui::Context) -> Result<()> {
        let song_count = self.state.songs.len();

        egui::Window::new("Songsel").show(ctx, |ui| {
            egui::Grid::new("songsel-grid")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| -> Result<()> {
                    if song_count > 0 {
                        {
                            let state = &mut self.state;
                            ui.label("Song");
                            if ui
                                .add(
                                    egui::DragValue::new(&mut state.selected_index)
                                        .clamp_range(0..=(song_count - 1))
                                        .speed(0.1),
                                )
                                .changed()
                            {
                                state.preview_countdown = 1500.0;

                                let set_song_idx: Function =
                                    self.lua.globals().get("set_index").unwrap();

                                set_song_idx.call::<_, i32>(state.selected_index + 1)?;
                            }
                        }
                        ui.end_row();
                        if ui.button("Start").clicked() {
                            self.suspend();
                            let state = &mut self.state;

                            let song = state
                                .songs
                                .get(state.selected_index as usize)
                                .cloned()
                                .unwrap();
                            let diff = state.selected_diff_index as usize;
                            let loader = self.song_provider.read().unwrap().load_song(
                                &SongDiffId::SongDiff(
                                    song.id.clone(),
                                    song.difficulties.read().unwrap()[diff].id.clone(),
                                ),
                            );
                            ensure!(self
                                .program_control
                                .as_ref()
                                .unwrap()
                                .send(ControlMessage::Song { diff, song, loader })
                                .is_ok());
                        }
                        ui.end_row();
                        Ok(())
                    } else {
                        ui.label("No songs");
                        Ok(())
                    }
                })
        });

        Ok(())
    }

    fn init(&mut self, app_control_tx: Sender<ControlMessage>) -> anyhow::Result<()> {
        self.update_lua()?;

        let lua_provider = self.services.get_required::<LuaProvider>();

        self.settings_dialog.init_lua(&lua_provider)?;
        self.program_control = Some(app_control_tx);
        lua_provider.register_libraries(self.lua.clone(), "songselect/songwheel.lua")?;
        lua_provider
            .register_libraries(self.background_lua.clone(), "songselect/background.lua")?;
        let mut bgm_amp = Arc::new(1_f32);
        let preview_playing = self.state.preview_finished.clone();
        let suspended = self.suspended.clone();
        self.mixer.add(owned_source(
            rodio::source::Zero::new(2, 44100) //TODO: Load something from skin audio
                .amplify(0.2)
                .pausable(false)
                .amplify(1.0)
                .periodic_access(Duration::from_millis(10), move |state| {
                    state
                        .inner_mut()
                        .set_paused(suspended.load(std::sync::atomic::Ordering::Relaxed));

                    let amp = Arc::get_mut(&mut bgm_amp).unwrap();
                    if preview_playing.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                        *amp += 1.0 / 50.0;
                    } else {
                        *amp -= 1.0 / 50.0;
                    }
                    *amp = amp.clamp(0.0, 1.0);
                    state.set_factor(*amp);
                }),
            self.sample_marker.clone(),
        ));

        Ok(())
    }

    fn tick(&mut self, _dt: f64, _knob_state: LaserState) -> Result<()> {
        if self.suspended.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }
        let song_advance_steps = (self.song_advance / KNOB_NAV_THRESHOLD).trunc() as i32;
        self.song_advance -= song_advance_steps as f32 * KNOB_NAV_THRESHOLD;

        let diff_advance_steps = (self.diff_advance / KNOB_NAV_THRESHOLD).trunc() as i32;
        self.diff_advance -= diff_advance_steps as f32 * KNOB_NAV_THRESHOLD;

        if song_advance_steps == 0
            && self.state.preview_countdown > 0.0
            && !self.state.songs.is_empty()
        {
            if self.state.preview_countdown < _dt {
                //Start playing preview
                //TODO: Reduce nesting
                let song_id = &self.state.songs[self.state.selected_index as usize].id;
                let song_id_u64 = song_id.as_u64();
                if self
                    .state
                    .preview_playing
                    .load(std::sync::atomic::Ordering::SeqCst)
                    != song_id_u64
                {
                    match self.song_provider.read().unwrap().get_preview(song_id) {
                        Ok((preview, skip, duration)) => {
                            profile_scope!("Start Preview");
                            self.state
                                .preview_finished
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                            self.state
                                .preview_playing
                                .store(song_id_u64, std::sync::atomic::Ordering::Relaxed);
                            let current_preview = self.state.preview_playing.clone();
                            let mut amp = Arc::new(1_f32);
                            let mixer = self.mixer.clone();
                            let owner = self.sample_marker.clone();
                            let preview_finish_signal = self.state.preview_finished.clone();
                            let suspended = self.suspended.clone();
                            _ =
                                poll_promise::Promise::spawn_thread("queue preview", move || {
                                    let source = take_duration_fade(
                                        rodio::source::Source::skip_duration(preview, skip)
                                            .pausable(false)
                                            .stoppable(),
                                        duration,
                                        Duration::from_millis(500),
                                        preview_finish_signal,
                                    )
                                    .fade_in(Duration::from_millis(500))
                                    .amplify(1.0)
                                    .periodic_access(Duration::from_millis(10), move |state| {
                                        state
                                            .inner_mut()
                                            .inner_mut()
                                            .inner_mut()
                                            .inner_mut()
                                            .set_paused(
                                                suspended
                                                    .load(std::sync::atomic::Ordering::Relaxed),
                                            );

                                        let amp = Arc::get_mut(&mut amp).unwrap();
                                        let current_preview = current_preview
                                            .load(std::sync::atomic::Ordering::Relaxed);
                                        if current_preview != song_id_u64 {
                                            *amp -= 1.0 / 50.0;
                                            if *amp < 0.0 {
                                                state.inner_mut().inner_mut().inner_mut().stop();
                                            }
                                        } else if *amp < 1.0 {
                                            *amp += 1.0 / 50.0;
                                        }
                                        state.set_factor(amp.clamp(0.0, 1.0));
                                    });

                                    mixer.as_ref().add(owned_source(source, owner));
                                });
                        }
                        Err(e) => warn!("Could not load preview: {e:?}"),
                    }
                }
            }
            self.state.preview_countdown -= _dt;
        } else if song_advance_steps != 0 {
            self.state.preview_countdown = 1500.0;
        }

        let mut songs_dirty = false;
        let mut index_dirty = false;

        while let Ok(provider_event) = self.song_events.try_recv() {
            songs_dirty = true;
            match provider_event {
                SongProviderEvent::SongsAdded(new_songs) => {
                    self.score_provider
                        .read()
                        .unwrap()
                        .init_scores(&mut new_songs.iter())?;
                    self.state.songs.append(new_songs)
                }
                SongProviderEvent::SongsRemoved(removed_ids) => {
                    self.state.songs.remove(removed_ids)
                }
                SongProviderEvent::OrderChanged(order) => {
                    let current_index = self.state.selected_index;

                    let id = self
                        .state
                        .songs
                        .get(self.state.selected_index as usize)
                        .map(|x| x.id.clone())
                        .unwrap_or_default();

                    self.state.songs.set_order(order);
                    self.state.selected_index =
                        self.state.songs.find_index(id).unwrap_or_default() as _;

                    index_dirty = self.state.selected_index != current_index;
                }
            }
        }

        while let Ok(score_event) = self.score_events.try_recv() {
            songs_dirty = true;
            match score_event {
                ScoreProviderEvent::NewScore(id, score) => {
                    self.song_provider.write().unwrap().add_score(id, score);
                }
            }
        }

        if songs_dirty {
            self.update_lua()?;

            if index_dirty {
                let set_song_idx: Function = self.lua.globals().get("set_index").unwrap();
                set_song_idx.call::<_, i32>(self.state.selected_index + 1)?;
            }

            let diff = self.state.selected_diff_index;
            self.state.selected_diff_index =
                self.state
                    .songs
                    .get(self.state.selected_index as usize)
                    .map(|s| s.difficulties.read().unwrap().len().saturating_sub(1))
                    .unwrap_or_default()
                    .min(self.state.selected_diff_index as usize) as _;

            if diff != self.state.selected_diff_index {
                let set_diff_idx: Function = self.lua.globals().get("set_diff").unwrap();
                set_diff_idx.call::<_, ()>(self.state.selected_diff_index + 1)?;
            }
        }

        if !self.state.songs.is_empty() {
            self.state.selected_index = (self.state.selected_index + song_advance_steps)
                .rem_euclid(self.state.songs.len() as i32);
            let song_idx = self.state.selected_index as usize;
            let song_idx = self.state.songs[song_idx].id.as_u64();
            self.song_provider
                .write()
                .unwrap()
                .set_current_index(song_idx as _);

            if song_advance_steps != 0 {
                let set_song_idx: Function = self.lua.globals().get("set_index").unwrap();

                set_song_idx.call::<_, ()>(self.state.selected_index + 1)?;
            }

            if diff_advance_steps != 0 || song_advance_steps != 0 {
                let prev_diff = self.state.selected_diff_index;
                let song = &self.state.songs[self.state.selected_index as usize];
                self.state.selected_diff_index =
                    (self.state.selected_diff_index + diff_advance_steps).clamp(
                        0,
                        song.difficulties.read().unwrap().len().saturating_sub(1) as _,
                    );

                if prev_diff != self.state.selected_diff_index {
                    let set_diff_idx: Function = self.lua.globals().get("set_diff").unwrap();
                    set_diff_idx.call::<_, ()>(self.state.selected_diff_index + 1)?;
                }
            }
        }

        Ok(())
    }

    fn on_event(&mut self, event: &Event<UscInputEvent>) {
        if self.settings_dialog.show {
            if let Event::UserEvent(e) = event {
                self.settings_dialog.on_input(e);
            }

            return;
        }

        if let Event::WindowEvent {
            event:
                WindowEvent::KeyboardInput {
                    input:
                        KeyboardInput {
                            state: ElementState::Pressed,
                            virtual_keycode: Some(VirtualKeyCode::Tab),
                            ..
                        },
                    ..
                },
            ..
        } = event
        {
            self.state.search_input_active = !self.state.search_input_active;
            self.input_state
                .set_text_input_active(self.state.search_input_active);
            _ = self.update_lua();
            return;
        }

        if self.state.search_input_active {
            //Text input handling
            let mut updated = true;
            match event {
                Event::WindowEvent {
                    window_id: _,
                    event: WindowEvent::ReceivedCharacter(char),
                } if !char.is_control() => {
                    self.state.search_text.push(*char);
                }
                Event::WindowEvent {
                    window_id: _,
                    event: WindowEvent::Ime(Ime::Commit(s)),
                } => self.state.search_text.push_str(s.as_str()),
                Event::WindowEvent {
                    event:
                        WindowEvent::KeyboardInput {
                            input:
                                KeyboardInput {
                                    state: ElementState::Pressed,
                                    virtual_keycode: Some(VirtualKeyCode::Back),
                                    ..
                                },
                            ..
                        },
                    ..
                } => {
                    self.state.search_text.pop();
                }
                _ => {
                    updated = false;
                }
            }

            if updated {
                _ = self.update_lua();
                self.song_provider
                    .write()
                    .unwrap()
                    .set_search(&self.state.search_text);
            }
        }

        if let Event::UserEvent(UscInputEvent::Laser(ls, _time)) = event {
            self.song_advance += LaserAxis::from(ls.get(kson::Side::Right)).delta;
            self.diff_advance += LaserAxis::from(ls.get(kson::Side::Left)).delta;
        }
    }

    fn on_button_pressed(
        &mut self,
        button: crate::button_codes::UscButton,
        _timestamp: SystemTime,
    ) {
        if self.settings_dialog.show {
            self.settings_dialog.on_button_press(button);
            return;
        }

        match button {
            UscButton::Start => {
                let state = &self.state;
                let song = self.state.songs.get(state.selected_index as usize).cloned();

                if let (Some(pc), Some(song)) = (&self.program_control, song) {
                    let diff = state.selected_diff_index as usize;
                    let loader =
                        self.song_provider
                            .read()
                            .unwrap()
                            .load_song(&SongDiffId::SongDiff(
                                song.id.clone(),
                                song.difficulties.read().unwrap()[diff].id.clone(),
                            ));
                    _ = pc.send(ControlMessage::Song { diff, loader, song });
                }
            }
            UscButton::FX(s) => {
                let press_time = std::time::SystemTime::now();

                if let Some(other_press_time) =
                    self.input_state.is_button_held(UscButton::FX(s.opposite()))
                {
                    let detla_ms = press_time
                        .duration_since(other_press_time)
                        .unwrap()
                        .as_millis();
                    if detla_ms < 100 {
                        self.settings_dialog.show = true;
                    }
                }
            }
            _ => (),
        }
    }

    fn suspend(&mut self) {
        self.suspended
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn resume(&mut self) {
        self.suspended
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    fn closed(&self) -> bool {
        self.closed
    }

    fn name(&self) -> &str {
        "Song Select"
    }
}

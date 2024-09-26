use anyhow::{anyhow, ensure, Result};
use di::{RefMut, ServiceProvider};
use game_loop::winit::event::{ElementState, Event, Ime, WindowEvent};
use itertools::Itertools;
use kson_rodio_sources::owned_source::{self, owned_source};
use log::warn;
use puffin::profile_function;
use rodio::Source;
use serde::Serialize;
use serde_json::json;
use std::{
    fmt::Debug,
    ops::Add,
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize},
        mpsc::{self, Receiver, Sender},
        Arc, RwLock,
    },
    time::{Duration, SystemTime},
};
use tealr::{
    mlu::{
        mlua::{self, Function, Lua, LuaSerdeExt},
        TealData, UserData,
    },
    SingleType, ToTypename,
};
use winit::{
    event::KeyEvent,
    keyboard::{Key, NamedKey},
};

use crate::{
    async_service::AsyncService,
    button_codes::{LaserAxis, LaserState, UscButton, UscInputEvent},
    config::GameConfig,
    game_main::AutoPlay,
    help::await_task,
    input_state::InputState,
    lua_service::LuaProvider,
    results::Score,
    scene::{Scene, SceneData},
    settings_dialog::SettingsDialog,
    song_provider::{
        self, DiffId, ScoreProvider, ScoreProviderEvent, SongDiffId, SongFilter, SongFilterType,
        SongId, SongProvider, SongProviderEvent, SongSort,
    },
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
            diff.jacket_path
                .clone()
                .into_os_string()
                .into_string()
                .map_err(|_| mlua::Error::external("Bad path"))
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
            Ok(song.difficulties.read().expect("Lock error").clone())
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuState {
    Songs,
    Levels,
    Folders,
    Sorting,
}

pub struct SongSelectScene {
    state: Box<SongSelect>,
    menu_state: MenuState,
    lua: Rc<Lua>,
    background_lua: Rc<Lua>,
    program_control: Option<Sender<ControlMessage>>,
    song_advance: f32,
    diff_advance: f32,
    suspended: Arc<AtomicBool>,
    closed: bool,
    mixer: RuscMixer,
    sample_owner: owned_source::Marker,
    settings_dialog: SettingsDialog,
    settings_closed: SystemTime,
    input_state: InputState,
    services: ServiceProvider,
    song_provider: RefMut<dyn SongProvider>,
    song_events: bus::BusReader<SongProviderEvent>,
    score_events: bus::BusReader<ScoreProviderEvent>,
    score_provider: RefMut<dyn ScoreProvider>,
    async_worker: RefMut<AsyncService>,
    sort_lua: Rc<Lua>,
    filter_lua: Rc<Lua>,
    level_filter: u8,
    folder_filter_index: usize,
    sort_index: usize,
    filters: Vec<song_provider::SongFilterType>,
    sorts: Vec<song_provider::SongSort>,
    auto_rx: Receiver<crate::game_main::AutoPlay>,
}

impl SongSelectScene {
    pub fn new(mut song_select: Box<SongSelect>, services: ServiceProvider) -> Self {
        let sample_owner = owned_source::Marker::new();
        let input_state = InputState::clone(&services.get_required());
        let song_provider: RefMut<dyn SongProvider> = services.get_required();
        let score_provider: RefMut<dyn ScoreProvider> = services.get_required();
        let score_events = score_provider.write().expect("Lock error").subscribe();
        let song_events = song_provider.write().expect("Lock error").subscribe();
        let initial_songs = song_provider.write().expect("Lock error").get_all();
        _ = score_provider
            .write()
            .expect("Lock error")
            .init_scores(&mut initial_songs.iter());
        song_select.songs.add(initial_songs, vec![]);
        let (auto_tx, auto_rx) = mpsc::channel();
        Self {
            filter_lua: LuaProvider::new_lua(),
            sort_lua: LuaProvider::new_lua(),
            background_lua: LuaProvider::new_lua(),
            lua: LuaProvider::new_lua(),
            state: song_select,
            program_control: None,
            diff_advance: 0.0,
            song_advance: 0.0,
            suspended: Arc::new(AtomicBool::new(false)),
            closed: false,
            mixer: services.get_required(),
            sample_owner,
            input_state: input_state.clone(),
            settings_dialog: SettingsDialog::general_settings(
                input_state,
                services.create_scope(),
                auto_tx,
            ),
            async_worker: services.get_required(),
            song_events,
            score_events,
            song_provider,
            score_provider,
            services,
            menu_state: MenuState::Songs,
            level_filter: 0,
            folder_filter_index: 0,
            sort_index: 0,
            filters: vec![],
            sorts: vec![],
            settings_closed: SystemTime::UNIX_EPOCH,
            auto_rx,
        }
    }

    fn on_search(&mut self) {
        _ = self.update_lua();
        self.song_provider
            .write()
            .expect("Lock error")
            .set_search(&self.state.search_text);
    }

    fn update_lua(&self) -> anyhow::Result<()> {
        Ok(self
            .lua
            .globals()
            .set("songwheel", self.lua.to_value(&self.state)?)?)
    }

    fn update_filter_sort_lua(&self) -> anyhow::Result<(Vec<SongFilterType>, Vec<SongSort>)> {
        let (filters, sorts) = {
            let sp = self.song_provider.read().expect("Lock error");
            (sp.get_available_filters(), sp.get_available_sorts())
        };

        self.sort_lua
            .globals()
            .set("sorts", sorts.iter().map(ToString::to_string).collect_vec())?;

        self.filter_lua.globals().set(
            "filters",
            self.filter_lua.to_value(&json!({
                "folder": filters.iter().map(|x| x.to_string()).collect_vec(),
                "level": (0..=20).map(|x| if x == 0 {"None".to_owned()} else {format!("Level: {x}")}).collect_vec(),
            }))?,
        )?;

        let set_selection: Function = self.filter_lua.globals().get("set_selection")?;
        set_selection.call((self.level_filter + 1, false))?;
        set_selection.call((self.folder_filter_index + 1, true))?;

        Ok((filters, sorts))
    }

    fn start_preview(&mut self) {
        let song_id = self.state.songs[self.state.selected_index as usize]
            .id
            .clone();
        let services = self.services.create_scope();

        let suspended = self.suspended.clone();
        let preview_playing = self.state.preview_playing.clone();
        let preview_finished = self.state.preview_finished.clone();
        let owner = self.sample_owner.clone();
        let mixer = self.mixer.clone();

        if preview_playing.load(std::sync::atomic::Ordering::Relaxed) == song_id.as_u64() {
            return;
        }

        self.async_worker.read().unwrap().run(async move {
            let preview = {
                let song_provider = services.get_required_mut::<dyn SongProvider>();
                let preview = song_provider.read().unwrap().get_preview(&song_id);
                preview
            };

            let (preview, skip, duration) = match await_task(preview).await {
                Ok(e) => e,
                Err(e) => {
                    warn!("Could not load preview: {e}");
                    return;
                }
            };

            add_preview_source(
                preview,
                skip,
                duration,
                suspended,
                preview_playing,
                preview_finished,
                &owner,
                song_id.as_u64(),
                mixer,
            );
        });
    }

    fn start_song(&mut self, autoplay: AutoPlay) {
        let state = &self.state;
        let song = self.state.songs.get(state.selected_index as usize).cloned();

        if let (Some(pc), Some(song)) = (&self.program_control, song) {
            let diff = state.selected_diff_index as usize;
            let song_diff = SongDiffId::SongDiff(song.id.clone(), {
                song.difficulties.read().expect("Lock error")[diff]
                    .id
                    .clone()
            });
            match self
                .song_provider
                .read()
                .expect("Lock error")
                .load_song(&song_diff)
            {
                Ok(loader) => {
                    GameConfig::get_mut().song_select.last_played = song_diff;
                    self.async_worker.read().unwrap().save_config();
                    _ = pc.send(ControlMessage::Song {
                        diff,
                        loader,
                        song: song.clone(),
                        autoplay,
                    });
                }
                Err(err) => {
                    log::warn!("Failed to load song: {err}");
                }
            };
        }
    }
}

fn add_preview_source<T: Source<Item = f32> + Send + 'static>(
    preview: T,
    skip: Duration,
    duration: Duration,
    suspended: Arc<AtomicBool>,
    preview_playing: Arc<AtomicU64>,
    preview_finished: Arc<AtomicUsize>,
    owner: &owned_source::Marker,
    song_id_u64: u64,
    mixer: RuscMixer,
) {
    let mut amp = 1.0f32;
    preview_playing.store(song_id_u64, std::sync::atomic::Ordering::Relaxed);
    preview_finished.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let source = take_duration_fade(
        rodio::source::Source::skip_duration(preview, skip)
            .pausable(false)
            .stoppable(),
        duration,
        Duration::from_millis(500),
        preview_finished,
    )
    .fade_in(Duration::from_millis(500))
    .amplify(1.0)
    .periodic_access(Duration::from_millis(10), move |state| {
        state
            .inner_mut()
            .inner_mut()
            .inner_mut()
            .inner_mut()
            .set_paused(suspended.load(std::sync::atomic::Ordering::Relaxed));

        let amp = &mut amp;
        let current_preview = preview_playing.load(std::sync::atomic::Ordering::Relaxed);
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
}

impl Scene for SongSelectScene {
    fn render_ui(&mut self, dt: f64) -> Result<()> {
        profile_function!();
        let render_bg: Function = self.background_lua.globals().get("render")?;
        render_bg.call(dt / 1000.0)?;

        let render_wheel: Function = self.lua.globals().get("render")?;
        render_wheel.call(dt / 1000.0)?;

        let render_filters: Function = self.filter_lua.globals().get("render")?;
        render_filters.call((
            dt / 1000.0,
            matches!(self.menu_state, MenuState::Folders | MenuState::Levels),
        ))?;

        let render_sorting: Function = self.sort_lua.globals().get("render")?;
        render_sorting.call((dt / 1000.0, self.menu_state == MenuState::Sorting))?;

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
                    ui.label(format!("Menu state {:?}", self.menu_state));
                    ui.end_row();

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

                                let set_song_idx: Function = self.lua.globals().get("set_index")?;

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
                                .ok_or(anyhow!("Selected index not in collection"))?;
                            let diff = state.selected_diff_index as usize;
                            let loader = self.song_provider.read().expect("Lock error").load_song(
                                &SongDiffId::SongDiff(
                                    song.id.clone(),
                                    song.difficulties.read().expect("Lock error")[diff]
                                        .id
                                        .clone(),
                                ),
                            )?;
                            ensure!(self
                                .program_control
                                .as_ref()
                                .ok_or(anyhow!("Program control not set"))?
                                .send(ControlMessage::Song {
                                    diff,
                                    song,
                                    loader,
                                    autoplay: crate::game_main::AutoPlay::None
                                })
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

        lua_provider.register_libraries(self.filter_lua.clone(), "songselect/filterwheel.lua")?;
        lua_provider.register_libraries(self.sort_lua.clone(), "songselect/sortwheel.lua")?;
        (self.filters, self.sorts) = self.update_filter_sort_lua()?;

        let mut bgm_amp = 1_f32;
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

                    let amp = &mut bgm_amp;
                    if preview_playing.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                        *amp += 1.0 / 50.0;
                    } else {
                        *amp -= 1.0 / 50.0;
                    }
                    *amp = amp.clamp(0.0, 1.0);
                    state.set_factor(*amp);
                }),
            &self.sample_owner,
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

        // Tick song audio preview
        if song_advance_steps == 0
            && self.state.preview_countdown > 0.0
            && !self.state.songs.is_empty()
        {
            if self.state.preview_countdown <= _dt {
                //Start playing preview
                self.start_preview();
            }
            self.state.preview_countdown -= _dt;
        } else if song_advance_steps != 0 {
            self.state.preview_countdown = 1500.0;
        }

        let mut songs_dirty = false;
        let mut index_dirty = false;
        let had_no_songs = self.state.songs.is_empty();
        let selected_index: SongId = self
            .state
            .songs
            .get(self.state.selected_index as _)
            .map(|x| x.id.clone())
            .unwrap_or_default();

        while let Ok(provider_event) = self.song_events.try_recv() {
            songs_dirty = true;
            match provider_event {
                SongProviderEvent::SongsAdded(new_songs) => {
                    self.score_provider
                        .read()
                        .expect("Lock error")
                        .init_scores(&mut new_songs.iter())?;
                    self.state.songs.append(new_songs)
                }
                SongProviderEvent::SongsRemoved(removed_ids) => {
                    if removed_ids.contains(&selected_index) {
                        self.state.selected_index = 0;
                        index_dirty = true;
                    }
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
                        self.state.songs.find_index(&id).unwrap_or_default() as _;

                    index_dirty = self.state.selected_index != current_index;
                }
            }
        }

        while let Ok(score_event) = self.score_events.try_recv() {
            songs_dirty = true;
            match score_event {
                ScoreProviderEvent::NewScore(id, score) => {
                    self.song_provider
                        .write()
                        .expect("Lock error")
                        .add_score(id, score);
                }
            }
        }

        if songs_dirty {
            self.update_lua()?;

            if had_no_songs {
                if let Some(id) = GameConfig::get().song_select.last_played.get_song() {
                    self.state.selected_index =
                        self.state.songs.find_index(id).unwrap_or_default() as _;

                    index_dirty = true;
                }
            }

            if index_dirty {
                let set_song_idx: Function = self.lua.globals().get("set_index")?;
                set_song_idx.call::<_, i32>(self.state.selected_index + 1)?;
            }

            let diff = self.state.selected_diff_index;
            self.state.selected_diff_index =
                self.state
                    .songs
                    .get(self.state.selected_index as usize)
                    .map(|s| {
                        s.difficulties
                            .read()
                            .expect("Lock error")
                            .len()
                            .saturating_sub(1)
                    })
                    .unwrap_or_default()
                    .min(self.state.selected_diff_index as usize) as _;

            if diff != self.state.selected_diff_index {
                let set_diff_idx: Function = self.lua.globals().get("set_diff")?;
                set_diff_idx.call::<_, ()>(self.state.selected_diff_index + 1)?;
            }
        }

        match self.menu_state {
            MenuState::Songs => {
                if !self.state.songs.is_empty() {
                    self.state.selected_index = (self.state.selected_index + song_advance_steps)
                        .rem_euclid(self.state.songs.len() as i32);
                    let song_idx = self.state.selected_index as usize;
                    let song_idx = self.state.songs[song_idx].id.as_u64();
                    self.song_provider
                        .write()
                        .expect("Lock error")
                        .set_current_index(song_idx as _);

                    if song_advance_steps != 0 {
                        let set_song_idx: Function = self.lua.globals().get("set_index")?;

                        set_song_idx.call::<_, ()>(self.state.selected_index + 1)?;
                    }

                    if diff_advance_steps != 0 || song_advance_steps != 0 {
                        let prev_diff = self.state.selected_diff_index;
                        let song = &self.state.songs[self.state.selected_index as usize];
                        self.state.selected_diff_index =
                            (self.state.selected_diff_index + diff_advance_steps).clamp(
                                0,
                                song.difficulties
                                    .read()
                                    .expect("Lock error")
                                    .len()
                                    .saturating_sub(1) as _,
                            );

                        if prev_diff != self.state.selected_diff_index {
                            let set_diff_idx: Function = self.lua.globals().get("set_diff")?;
                            set_diff_idx.call::<_, ()>(self.state.selected_diff_index + 1)?;
                        }
                    }
                }
            }
            MenuState::Sorting => {
                if !self.sorts.is_empty() {
                    self.sort_index = diff_advance_steps
                        .add(song_advance_steps)
                        .add(self.sort_index as i32)
                        .rem_euclid(self.sorts.len() as _)
                        as _;

                    if (diff_advance_steps + song_advance_steps) != 0 {
                        self.song_provider
                            .write()
                            .expect("Lock error")
                            .set_sort(self.sorts[self.sort_index]);
                        let set_selection: Function =
                            self.sort_lua.globals().get("set_selection")?;
                        set_selection.call(self.sort_index + 1)?;
                    }
                }
            }
            MenuState::Levels => {
                self.level_filter = (diff_advance_steps + song_advance_steps)
                    .add(self.level_filter as i32)
                    .rem_euclid(21) as _;
                if (diff_advance_steps + song_advance_steps) != 0 {
                    self.song_provider
                        .write()
                        .expect("Lock error")
                        .set_filter(SongFilter::new(
                            self.filters[self.folder_filter_index].clone(),
                            self.level_filter,
                        ));
                    let set_selection: Function = self.filter_lua.globals().get("set_selection")?;
                    set_selection.call((self.level_filter + 1, false))?;
                }
            }
            MenuState::Folders => {
                if !self.filters.is_empty() {
                    self.folder_filter_index = (diff_advance_steps + song_advance_steps)
                        .add(self.folder_filter_index as i32)
                        .rem_euclid(self.filters.len() as _)
                        as _;
                    if (diff_advance_steps + song_advance_steps) != 0 {
                        self.song_provider.write().expect("Lock error").set_filter(
                            SongFilter::new(
                                self.filters[self.folder_filter_index].clone(),
                                self.level_filter,
                            ),
                        );
                        let set_selection: Function =
                            self.filter_lua.globals().get("set_selection")?;
                        set_selection.call((self.folder_filter_index + 1, true))?;
                    }
                }
            }
        }

        if let Ok(autoplay) = self.auto_rx.try_recv() {
            self.start_song(autoplay);
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
                    event:
                        KeyEvent {
                            state: ElementState::Pressed,
                            logical_key: Key::Named(NamedKey::Tab),
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

        if let Event::UserEvent(UscInputEvent::ClientEvent(e)) = event {
            match e {
                crate::companion_interface::ClientEvent::SetSearch(s) => {
                    self.state.search_text = s.to_string();
                    self.on_search();
                }
                crate::companion_interface::ClientEvent::SetLevelFilter(x) => {
                    self.level_filter = *x;
                    self.song_provider
                        .write()
                        .unwrap()
                        .set_filter(SongFilter::new(
                            self.filters[self.folder_filter_index].clone(),
                            self.level_filter,
                        ));
                    _ = self.update_lua();
                    _ = self.update_filter_sort_lua();
                }
                crate::companion_interface::ClientEvent::SetSongFilterType(song_filter_type) => {
                    if let Some(pos) = self
                        .filters
                        .iter()
                        .find_position(|x| **x == *song_filter_type)
                    {
                        self.folder_filter_index = pos.0;
                        self.song_provider
                            .write()
                            .unwrap()
                            .set_filter(SongFilter::new(
                                song_filter_type.clone(),
                                self.level_filter,
                            ));
                        _ = self.update_lua();
                        _ = self.update_filter_sort_lua();
                    }
                }
                crate::companion_interface::ClientEvent::SetSongSort(song_sort) => {
                    if let Some(pos) = self.sorts.iter().find_position(|x| **x == *song_sort) {
                        self.sort_index = pos.0;
                        self.song_provider
                            .write()
                            .unwrap()
                            .set_sort(song_sort.clone());
                        _ = self.update_lua();
                        _ = self.update_filter_sort_lua();
                    }
                }
                _ => {}
            }
        }

        if self.state.search_input_active {
            //Text input handling
            let mut updated = true;
            match event {
                Event::WindowEvent {
                    window_id: _,
                    event:
                        WindowEvent::KeyboardInput {
                            event:
                                KeyEvent {
                                    text: Some(text),
                                    state: ElementState::Pressed,
                                    ..
                                },
                            ..
                        },
                } if !text.chars().any(char::is_control) => {
                    self.state.search_text += text.as_str();
                }
                Event::WindowEvent {
                    window_id: _,
                    event: WindowEvent::Ime(Ime::Commit(s)),
                } => self.state.search_text.push_str(s.as_str()),
                Event::WindowEvent {
                    event:
                        WindowEvent::KeyboardInput {
                            event:
                                KeyEvent {
                                    state: ElementState::Pressed,
                                    logical_key: Key::Named(NamedKey::Backspace),
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
                self.on_search();
            }
        }

        if let Event::UserEvent(UscInputEvent::Laser(ls, _time)) = event {
            self.song_advance += LaserAxis::from(ls.get(kson::Side::Right)).delta;
            self.diff_advance += LaserAxis::from(ls.get(kson::Side::Left)).delta;
        }
    }

    fn on_button_pressed(&mut self, button: crate::button_codes::UscButton, timestamp: SystemTime) {
        if self.settings_dialog.show {
            self.settings_dialog.on_button_press(button);
            self.settings_closed = SystemTime::now();
            return;
        }

        // Ignore inputs for a short bit to avoid opening anything unintended
        if SystemTime::now()
            .duration_since(self.settings_closed)
            .expect("Clock error")
            .as_secs_f32()
            < 0.25
        {
            return;
        }

        match button {
            UscButton::Back if MenuState::Songs == self.menu_state => {
                self.closed = true;
            }
            UscButton::Start => {
                match self.menu_state {
                    MenuState::Songs => {
                        self.start_song(AutoPlay::None);
                    }
                    MenuState::Levels => {
                        self.menu_state = MenuState::Folders;
                    }
                    MenuState::Folders => {
                        self.menu_state = MenuState::Levels;
                    }
                    MenuState::Sorting => {}
                }

                if let MenuState::Folders | MenuState::Levels = self.menu_state {
                    if let Ok(set_mode) = self.filter_lua.globals().get::<_, Function>("set_mode") {
                        _ = set_mode.call::<_, ()>(self.menu_state == MenuState::Folders);
                    }
                }
            }
            UscButton::FX(s) => {
                if let Some(other_press_time) =
                    self.input_state.is_button_held(UscButton::FX(s.opposite()))
                {
                    let detla_ms = timestamp
                        .duration_since(other_press_time)
                        .unwrap_or_default()
                        .as_millis();
                    if detla_ms < 100 && self.menu_state == MenuState::Songs {
                        self.settings_dialog.show = true;
                    }
                }
            }
            _ => (),
        }
    }
    fn on_button_released(&mut self, button: UscButton, _timestamp: SystemTime) {
        // Ignore inputs for a short bit to avoid opening anything unintended

        if self.settings_dialog.show
            || SystemTime::now()
                .duration_since(self.settings_closed)
                .expect("Clock error")
                .as_secs_f32()
                < 0.25
        {
            return;
        }

        if let UscButton::FX(side) = button {
            self.menu_state = match (side, self.menu_state) {
                (kson::Side::Left, MenuState::Songs) => MenuState::Folders,
                (kson::Side::Left, MenuState::Levels) => MenuState::Songs,
                (kson::Side::Left, MenuState::Folders) => MenuState::Songs,
                (kson::Side::Left, MenuState::Sorting) => MenuState::Sorting,
                (kson::Side::Right, MenuState::Songs) => MenuState::Sorting,
                (kson::Side::Right, MenuState::Levels) => MenuState::Levels,
                (kson::Side::Right, MenuState::Folders) => MenuState::Folders,
                (kson::Side::Right, MenuState::Sorting) => MenuState::Songs,
            };

            if let MenuState::Folders | MenuState::Levels = self.menu_state {
                if let Ok(set_mode) = self.filter_lua.globals().get::<_, Function>("set_mode") {
                    _ = set_mode.call::<_, ()>(self.menu_state == MenuState::Folders);
                }
            }
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

    fn game_state(&self) -> crate::companion_interface::GameState {
        crate::companion_interface::GameState::SongSelect {
            search_string: self.state.search_text.clone().into(),
            level_filter: self.level_filter,
            folder_filter_index: self.folder_filter_index,
            sort_index: self.sort_index,
            filters: self.filters.clone(),
            sorts: self.sorts.clone(),
        }
    }
}

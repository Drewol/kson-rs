use anyhow::{ensure, Result};
use game_loop::winit::event::Event;
use generational_arena::Index;
use log::warn;
use puffin::{profile_function, profile_scope};
use rodio::{dynamic_mixer::DynamicMixerController, Source};
use serde::Serialize;
use std::{
    fmt::Debug,
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, AtomicUsize},
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex,
    },
    time::Duration,
};
use tealr::{
    mlu::{
        mlua::{Function, Lua, LuaSerdeExt},
        TealData, UserData,
    },
    TypeName,
};

use crate::{
    button_codes::{LaserAxis, LaserState, UscButton, UscInputEvent},
    config::GameConfig,
    input_state::InputState,
    results::Score,
    scene::{Scene, SceneData},
    song_provider::{
        FileSongProvider, NauticaSongProvider, ScoreProvider, SongProvider, SongProviderEvent,
    },
    sources::{
        bitcrush::bit_crusher, effected_part::effected_part, flanger::flanger,
        owned_source::owned_source,
    },
    take_duration_fade::take_duration_fade,
    ControlMessage, RuscMixer,
};

#[derive(Debug, TypeName, Clone, Serialize, UserData)]
#[serde(rename_all = "camelCase")]
pub struct Difficulty {
    pub jacket_path: PathBuf,
    pub level: u8,
    pub difficulty: u8, // 0 = nov, 1 = adv, etc.
    pub id: u64,        //unique static identifier
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
        fields.add_field_method_get("id", |_, diff| Ok(diff.id));
        fields.add_field_method_get("effector", |_, diff| Ok(diff.effector.clone()));
        fields.add_field_method_get("topBadge", |_, diff| Ok(diff.top_badge));
        fields.add_field_method_get("scores", |_, diff| Ok(diff.scores.clone()));
    }
}

#[derive(Debug, TypeName, UserData, Clone, Serialize, Default)]
pub struct Song {
    pub title: String,
    pub artist: String,
    pub bpm: String,                   //ex. "170-200"
    pub id: u64,                       //unique static identifier
    pub difficulties: Vec<Difficulty>, //array of all difficulties for this song
}

//Keep tealdata for generating type definitions
impl TealData for Song {
    fn add_fields<'lua, F: tealr::mlu::TealDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("title", |_, song| Ok(song.title.clone()));
        fields.add_field_method_get("artist", |_, song| Ok(song.artist.clone()));
        fields.add_field_method_get("bpm", |_, song| Ok(song.bpm.clone()));
        fields.add_field_method_get("id", |_, song| Ok(song.id));
        fields.add_field_method_get("difficulties", |_, song| Ok(song.difficulties.clone()));
    }
}

#[derive(Debug, Serialize, UserData)]
pub struct SongSelect {
    songs: Vec<Arc<Song>>,
    searchInputActive: bool, //true when the user is currently inputting search text
    searchText: String,      //current string used by the song search
    selected_index: i32,
    selected_diff_index: i32,
    #[serde(skip_serializing)]
    song_provider: Arc<Mutex<dyn SongProvider + Send>>,
    #[serde(skip_serializing)]
    score_provider: Arc<Mutex<dyn ScoreProvider + Send>>,
    preview_countdown: f64,
    preview_finished: Arc<AtomicUsize>,
    preview_playing: Arc<AtomicU64>,
}

impl TealData for SongSelect {
    fn add_fields<'lua, F: tealr::mlu::TealDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("songs", |_, _| Ok([] as [Song; 0]));
        fields.add_field_method_get("searchInputActive", |_, songwheel| {
            Ok(songwheel.searchInputActive)
        });
        fields.add_field_method_get("searchText", |_, songwheel| {
            Ok(songwheel.searchText.clone())
        });
        fields.add_field_method_get(
            "searchStatus",
            |_, _| -> Result<Option<String>, tealr::mlu::mlua::Error> { Ok(None) },
        );
    }
}

impl TypeName for SongSelect {
    fn get_type_parts() -> std::borrow::Cow<'static, [tealr::NamePart]> {
        use std::borrow::Cow;

        Cow::Borrowed(&[tealr::NamePart::Type(tealr::TealType {
            name: Cow::Borrowed("songwheel"),
            type_kind: tealr::KindOfType::External,
            generics: None,
        })])
    }
}

impl SongSelect {
    pub fn new() -> Self {
        let song_path = { GameConfig::get().unwrap().songs_path.clone() };

        let (song_provider, score_provider): (
            Arc<Mutex<dyn SongProvider + Send>>,
            Arc<Mutex<dyn ScoreProvider + Send>>,
        ) = if song_path == PathBuf::from("nautica") {
            (
                Arc::new(Mutex::new(NauticaSongProvider::new())),
                Arc::new(Mutex::new(crate::block_on!(FileSongProvider::new()))),
            )
        } else {
            let val = Arc::new(Mutex::new(crate::block_on!(FileSongProvider::new())));
            (val.clone(), val)
        };

        let mut songs = if let Some(SongProviderEvent::SongsAdded(songs)) =
            song_provider.lock().unwrap().poll()
        {
            songs
        } else {
            vec![]
        };

        songs.sort_by_key(|s| s.title.to_lowercase());

        Self {
            songs,
            searchInputActive: false,
            searchText: String::new(),
            selected_index: 0,
            selected_diff_index: 0,
            song_provider,
            score_provider,
            preview_countdown: 1500.0,
            preview_finished: Arc::new(AtomicUsize::new(0)),
            preview_playing: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl SceneData for SongSelect {
    fn make_scene(self: Box<Self>, _input_state: Arc<InputState>) -> Box<dyn Scene> {
        Box::new(SongSelectScene::new(self))
    }
}

pub struct SongSelectScene {
    state: Box<SongSelect>,
    lua: Rc<Lua>,
    background_lua: Rc<Lua>,
    program_control: Option<Sender<ControlMessage>>,
    song_advance: f32,
    diff_advance: f32,
    suspended: bool,
    closed: bool,
    mixer: Option<RuscMixer>,
    sample_owner: Receiver<()>,
    sample_marker: Sender<()>,
}

impl SongSelectScene {
    pub fn new(song_select: Box<SongSelect>) -> Self {
        let (sample_marker, sample_owner) = channel();
        Self {
            background_lua: Rc::new(Lua::new()),
            lua: Rc::new(Lua::new()),
            state: song_select,
            program_control: None,
            diff_advance: 0.0,
            song_advance: 0.0,
            suspended: false,
            closed: false,
            mixer: None,
            sample_marker,
            sample_owner,
        }
    }
}

impl Scene for SongSelectScene {
    fn render_ui(&mut self, dt: f64) -> Result<()> {
        profile_function!();
        let render_bg: Function = self.background_lua.globals().get("render")?;
        render_bg.call(dt / 1000.0)?;

        let render_wheel: Function = self.lua.globals().get("render")?;
        render_wheel.call(dt / 1000.0)?;

        Ok(())
    }

    fn is_suspended(&self) -> bool {
        self.suspended
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
                            let song = state.songs[state.selected_index as usize].clone();
                            let diff = state.selected_diff_index as usize;
                            let loader = state
                                .song_provider
                                .lock()
                                .unwrap()
                                .load_song(song.id, song.difficulties[diff].id);
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

    fn init(
        &mut self,
        load_lua: Rc<dyn Fn(Rc<Lua>, &'static str) -> anyhow::Result<Index>>,
        app_control_tx: Sender<ControlMessage>,
        mixer: Arc<DynamicMixerController<f32>>,
    ) -> anyhow::Result<()> {
        self.lua
            .globals()
            .set("songwheel", self.lua.to_value(&self.state)?)?;
        self.program_control = Some(app_control_tx);
        load_lua(self.lua.clone(), "songselect/songwheel.lua")?;
        load_lua(self.background_lua.clone(), "songselect/background.lua")?;
        let mut bgm_amp = Arc::new(1_f32);
        let preview_playing = self.state.preview_finished.clone();

        mixer.add(owned_source(
            rodio::source::SineWave::new(440.0) //TODO: Load something from skin audio
                .amplify(0.2)
                .amplify(1.0)
                .periodic_access(Duration::from_millis(10), move |state| {
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

        self.mixer = Some(mixer);
        Ok(())
    }

    fn tick(&mut self, _dt: f64, _knob_state: LaserState) -> Result<()> {
        if self.suspended {
            return Ok(());
        }
        const KNOB_NAV_THRESHOLD: f32 = std::f32::consts::PI / 3.0;
        let song_advance_steps = (self.song_advance / KNOB_NAV_THRESHOLD).trunc() as i32;
        self.song_advance -= song_advance_steps as f32 * KNOB_NAV_THRESHOLD;

        if song_advance_steps == 0
            && self.state.preview_countdown > 0.0
            && !self.state.songs.is_empty()
        {
            if self.state.preview_countdown < _dt {
                //Start playing preview
                //TODO: Reduce nesting
                let song_id = self.state.songs[self.state.selected_index as usize].id;
                if self
                    .state
                    .preview_playing
                    .load(std::sync::atomic::Ordering::SeqCst)
                    != song_id
                {
                    match self
                        .state
                        .song_provider
                        .lock()
                        .unwrap()
                        .get_preview(song_id)
                    {
                        Ok((preview, skip, duration)) => {
                            profile_scope!("Start Preview");
                            self.state
                                .preview_finished
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                            self.state
                                .preview_playing
                                .store(song_id, std::sync::atomic::Ordering::Relaxed);
                            let current_preview = self.state.preview_playing.clone();
                            let mut amp = Arc::new(1_f32);
                            let mixer = self.mixer.clone();
                            let owner = self.sample_marker.clone();
                            let preview_finish_signal = self.state.preview_finished.clone();
                            _ = poll_promise::Promise::spawn_thread("queue preview", move || {
                                let source = take_duration_fade(
                                    rodio::source::Source::skip_duration(preview, skip).stoppable(),
                                    duration,
                                    Duration::from_millis(500),
                                    preview_finish_signal,
                                )
                                .fade_in(Duration::from_millis(500))
                                .amplify(1.0)
                                .periodic_access(Duration::from_millis(10), move |state| {
                                    let amp = Arc::get_mut(&mut amp).unwrap();
                                    let current_preview =
                                        current_preview.load(std::sync::atomic::Ordering::Relaxed);
                                    if current_preview != song_id {
                                        *amp -= 1.0 / 50.0;
                                        if *amp < 0.0 {
                                            state.inner_mut().inner_mut().inner_mut().stop();
                                        }
                                    } else if *amp < 1.0 {
                                        *amp += 1.0 / 50.0;
                                    }
                                    state.set_factor(amp.clamp(0.0, 1.0));
                                })
                                .buffered();
                                let bit_source = source.clone();
                                let source = effected_part(
                                    source.clone(),
                                    flanger(
                                        source,
                                        Duration::from_millis(4),
                                        Duration::from_millis(2),
                                        0.5,
                                    ),
                                    Duration::from_millis(2000),
                                    Duration::from_millis(2000),
                                );

                                let source = effected_part(
                                    source,
                                    bit_crusher(bit_source, 45),
                                    Duration::from_millis(4000),
                                    Duration::from_millis(2000),
                                );

                                mixer.as_ref().unwrap().add(owned_source(source, owner));
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

        let state = &mut self.state;
        let mut songs_dirty = false;
        while let Some(provider_event) = state.song_provider.lock().unwrap().poll() {
            songs_dirty = true;
            match provider_event {
                SongProviderEvent::SongsAdded(mut new_songs) => state.songs.append(&mut new_songs),
                SongProviderEvent::SongsRemoved(removed_ids) => {
                    state.songs.retain(|s| !removed_ids.contains(&s.id))
                }
                SongProviderEvent::OrderChanged(_) => todo!(),
            }
        }

        if songs_dirty {
            self.lua
                .globals()
                .set("songwheel", self.lua.to_value(state.as_ref())?)?;
        }

        if !state.songs.is_empty() {
            state.selected_index =
                (state.selected_index + song_advance_steps).rem_euclid(state.songs.len() as i32);
            let song_idx = state.selected_index as usize;
            let song_id = state.songs[song_idx].id;
            state
                .song_provider
                .lock()
                .unwrap()
                .set_current_index(song_id);

            if song_advance_steps != 0 {
                let set_song_idx: Function = self.lua.globals().get("set_index").unwrap();

                set_song_idx.call::<_, ()>(state.selected_index + 1)?;
            }
        }

        Ok(())
    }

    fn on_event(&mut self, event: &Event<UscInputEvent>) {
        if let Event::UserEvent(UscInputEvent::Laser(ls)) = event {
            self.song_advance += LaserAxis::from(ls.get(kson::Side::Right)).delta;
            self.diff_advance += LaserAxis::from(ls.get(kson::Side::Left)).delta;
        }
    }

    fn on_button_pressed(&mut self, button: crate::button_codes::UscButton) {
        if let UscButton::Start = button {
            if let Some(pc) = &self.program_control {
                let state = &self.state;
                let song = state.songs[state.selected_index as usize].clone();
                let diff = state.selected_diff_index as usize;
                let loader = state
                    .song_provider
                    .lock()
                    .unwrap()
                    .load_song(song.id, song.difficulties[diff].id);
                pc.send(ControlMessage::Song { diff, loader, song });
            }
        }
    }

    fn suspend(&mut self) {
        self.suspended = true;
        self.state
            .preview_finished
            .store(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn resume(&mut self) {
        self.suspended = false;
        self.state
            .preview_finished
            .store(0, std::sync::atomic::Ordering::Relaxed);
        self.state
            .preview_playing
            .store(0, std::sync::atomic::Ordering::Relaxed);
        self.state.preview_countdown = 1500.0;
    }

    fn closed(&self) -> bool {
        self.closed
    }

    fn name(&self) -> &str {
        "Song Select"
    }
}

use std::{
    cell::Ref,
    collections::HashMap,
    fmt::Debug,
    fs::FileType,
    ops::Deref,
    path::PathBuf,
    rc::Rc,
    sync::{mpsc::Sender, Arc, Mutex},
};

use anyhow::Result;
use generational_arena::Index;
use kson::{Chart, Ksh};
use log::info;
use puffin::{profile_function, profile_scope};
use serde::Serialize;
use tealr::{
    mlu::{
        mlua::{AnyUserData, Function, Lua, LuaSerdeExt, ToLua, UserData},
        TealData, UserData,
    },
    TypeName,
};

use crate::{
    button_codes::{LaserAxis, LaserState, UscButton},
    scene::{Scene, SceneData},
    ControlMessage,
};

#[derive(Debug, TypeName, Clone, Serialize, UserData)]
#[serde(rename_all = "camelCase")]
pub struct Difficulty {
    jacket_path: PathBuf,
    level: u8,
    difficulty: u8, // 0 = nov, 1 = adv, etc.
    id: i32,        //unique static identifier
    effector: String,
    best_badge: i32,  //top badge for this difficulty
    scores: Vec<i32>, //array of all scores on this diff
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
        fields.add_field_method_get("bestBadge", |_, diff| Ok(diff.best_badge));
        fields.add_field_method_get("scores", |_, diff| Ok(diff.scores.clone()));
    }
}

#[derive(Debug, TypeName, UserData, Clone, Serialize)]
pub struct Song {
    title: String,
    artist: String,
    bpm: String,                   //ex. "170-200"
    id: i32,                       //unique static identifier
    path: PathBuf,                 //folder the song is stored in
    difficulties: Vec<Difficulty>, //array of all difficulties for this song
}

//Keep tealdata for generating type definitions
impl TealData for Song {
    fn add_fields<'lua, F: tealr::mlu::TealDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("title", |_, song| Ok(song.title.clone()));
        fields.add_field_method_get("artist", |_, song| Ok(song.artist.clone()));
        fields.add_field_method_get("bpm", |_, song| Ok(song.bpm.clone()));
        fields.add_field_method_get("id", |_, song| Ok(song.id));
        fields.add_field_method_get("path", |_, song| {
            Ok(song.path.clone().into_os_string().into_string().unwrap())
        });
        fields.add_field_method_get("difficulties", |_, song| Ok(song.difficulties.clone()));
    }
}

#[derive(Debug, Serialize, UserData)]
pub struct SongSelect {
    songs: Vec<Song>,
    searchInputActive: bool, //true when the user is currently inputting search text
    searchText: String,      //current string used by the song search
    selected_index: i32,
}

impl TealData for SongSelect {
    fn add_fields<'lua, F: tealr::mlu::TealDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("songs", |lua, songwheel| {
            profile_scope!("Cloning songs");
            songwheel.songs.clone().to_lua(lua)
        });
        fields.add_field_method_get("searchInputActive", |_, songwheel| {
            Ok(songwheel.searchInputActive)
        });
        fields.add_field_method_get("searchText", |_, songwheel| {
            Ok(songwheel.searchText.clone())
        });
        fields.add_field_method_get(
            "searchStatus",
            |_, songwheel| -> Result<Option<String>, tealr::mlu::mlua::Error> { Ok(None) },
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
    pub fn new(song_path: impl std::convert::AsRef<std::path::Path> + Debug) -> Self {
        info!("Loading songs from: {:?}", &song_path);
        let song_walker = walkdir::WalkDir::new(song_path);

        let charts = song_walker
            .into_iter()
            .filter_map(|a| a.ok())
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| {
                if let Ok(data) = std::fs::read_to_string(e.path()) {
                    Some((e, data))
                } else {
                    None
                }
            })
            .filter_map(|(dir, data)| {
                if let Ok(chart) = kson::Chart::from_ksh(&data) {
                    Some((dir, chart))
                } else {
                    None
                }
            });

        let song_folders = charts.fold(
            HashMap::<PathBuf, Vec<Chart>>::new(),
            |mut acc, (dir, chart)| {
                if let Some(parent_folder) = dir.path().parent() {
                    acc.entry(parent_folder.to_path_buf())
                        .and_modify(|v| v.push(chart.clone()))
                        .or_insert_with(|| vec![chart]);
                }
                acc
            },
        );

        let mut songs: Vec<Song> = song_folders
            .into_iter()
            .enumerate()
            .map(|(id, (song_folder, charts))| Song {
                title: charts[0].meta.title.clone(),
                artist: charts[0].meta.artist.clone(),
                bpm: charts[0].meta.disp_bpm.clone(),
                id: id as i32,
                path: song_folder.clone(),
                difficulties: charts
                    .iter()
                    .enumerate()
                    .map(|(id, c)| Difficulty {
                        best_badge: 0,
                        difficulty: c.meta.difficulty,
                        effector: c.meta.chart_author.clone(),
                        id: id as i32,
                        jacket_path: song_folder.join(&c.meta.jacket_filename),
                        level: c.meta.level,
                        scores: vec![99],
                    })
                    .collect(),
            })
            .collect();

        songs.sort_by_key(|s| s.title.to_lowercase());

        Self {
            songs,
            searchInputActive: false,
            searchText: String::new(),
            selected_index: 0,
        }
    }
}

impl SceneData for SongSelect {
    fn make_scene(self: Arc<Self>) -> Box<dyn Scene> {
        Box::new(SongSelectScene::new(
            Arc::try_unwrap(self).expect("SceneData not released by others"),
        ))
    }
}

pub struct SongSelectScene {
    state: Arc<Mutex<SongSelect>>,
    lua: Rc<Lua>,
    background_lua: Rc<Lua>,
    program_control: Option<Sender<ControlMessage>>,
    song_advance: f32,
    diff_advance: f32,
}

impl SongSelectScene {
    pub fn new(song_select: SongSelect) -> Self {
        Self {
            background_lua: Rc::new(Lua::new()),
            lua: Rc::new(Lua::new()),
            state: Arc::new(Mutex::new(song_select)),
            program_control: None,
            diff_advance: 0.0,
            song_advance: 0.0,
        }
    }
}

impl Scene for SongSelectScene {
    fn render(&mut self, dt: f64) -> Result<bool> {
        profile_function!();
        // let render_bg: Function = self.background_lua.globals().get("render")?;
        // render_bg.call(dt)?;

        let render_wheel: Function = self.lua.globals().get("render")?;
        render_wheel.call(dt / 1000.0)?;

        Ok(false)
    }

    fn is_suspended(&self) -> bool {
        false
    }

    fn debug_ui(&mut self, ctx: &three_d::egui::Context) -> Result<()> {
        use three_d::egui;
        let set_song_idx: Function = self.lua.globals().get("set_index").unwrap();
        if let Ok(state) = &mut self.state.lock() {
            let song_count = state.songs.len();

            egui::Window::new("Songsel").show(ctx, |ui| {
                egui::Grid::new("songsel-grid")
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        if song_count > 0 {
                            ui.label("Song");
                            if ui
                                .add(
                                    egui::DragValue::new(&mut state.selected_index)
                                        .clamp_range(0..=(song_count - 1))
                                        .speed(0.1),
                                )
                                .changed()
                            {
                                set_song_idx.call::<_, i32>(state.selected_index + 1);
                            }

                            ui.end_row()
                        } else {
                            ui.label("No songs");
                        }
                    })
            });
        }

        Ok(())
    }

    fn init(
        &mut self,
        load_lua: Box<dyn Fn(Rc<Lua>, &'static str) -> anyhow::Result<Index>>,
        app_control_tx: Sender<ControlMessage>,
    ) -> anyhow::Result<()> {
        self.lua
            .globals()
            .set("songwheel", self.lua.to_value(&self.state)?)?;
        self.program_control = Some(app_control_tx);
        load_lua(self.lua.clone(), "songselect/songwheel.lua")?;
        //load_lua(&self.background_lua, "songselect/background.lua")?;
        Ok(())
    }

    fn tick(&mut self, dt: f64, knob_state: LaserState) -> Result<bool> {
        self.song_advance += LaserAxis::from(knob_state.get(kson::Side::Right)).delta;
        self.diff_advance += LaserAxis::from(knob_state.get(kson::Side::Left)).delta;

        const KNOB_NAV_THRESHOLD: f32 = std::f32::consts::PI / 3.0;
        let song_advance_steps = (self.song_advance / KNOB_NAV_THRESHOLD).trunc() as i32;
        self.song_advance -= song_advance_steps as f32 * KNOB_NAV_THRESHOLD;
        if let Ok(state) = &mut self.state.lock() {
            if !state.songs.is_empty() {
                state.selected_index = (state.selected_index + song_advance_steps)
                    .rem_euclid(state.songs.len() as i32);
                if song_advance_steps != 0 {
                    let set_song_idx: Function = self.lua.globals().get("set_index").unwrap();
                    set_song_idx.call::<_, ()>(state.selected_index + 1);
                }
            }
        }

        Ok(false)
    }

    fn on_event(&mut self, event: &mut three_d::Event) {}

    fn on_button_pressed(&mut self, button: crate::button_codes::UscButton) {
        if let UscButton::Start = button {
            let state = self.state.lock().unwrap();
            if let Some(pc) = &self.program_control {
                pc.send(ControlMessage::Song(
                    state.songs[state.selected_index as usize].path.clone(),
                ))
                .unwrap();
            }
        }
    }

    fn suspend(&mut self) {}

    fn resume(&mut self) {}
}

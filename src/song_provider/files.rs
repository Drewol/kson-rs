use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::Arc,
};

use crate::{
    results::Score,
    songselect::{Difficulty, Song},
};

use super::{ScoreProvider, SongProvider, SongProviderEvent};
use itertools::Itertools;
use kson::{Chart, Ksh};
use log::info;
use rodio::Source;
use rusc_database::{LocalSongsDb, ScoreEntry};
use walkdir::DirEntry;

#[derive(Debug)]
pub struct FileSongProvider {
    all_songs: Vec<Arc<Song>>,
    new_songs: Vec<Arc<Song>>,
    difficulty_id_path_map: HashMap<u64, PathBuf>,
    events: VecDeque<SongProviderEvent>,
    database: rusc_database::LocalSongsDb,
}

impl From<ScoreEntry> for Score {
    fn from(value: ScoreEntry) -> Self {
        Score {
            gauge: value.gauge as f32,
            gauge_type: value.gauge_type as i32,
            gauge_option: value.gauge_opt as i32,
            mirror: value.mirror,
            random: value.random,
            auto_flags: value.auto_flags as i32,
            score: value.score as i32,
            perfects: value.crit as i32,
            goods: value.near as i32,
            misses: value.miss as i32,
            badge: 0, //TODO: Calculate
            timestamp: value.timestamp as i32,
            player_name: value.user_name,
            is_local: value.local_score,
        }
    }
}

impl FileSongProvider {
    pub async fn new() -> Self {
        let database = LocalSongsDb::new("./maps.db")
            .await
            .expect("Failed to open database");

        let mut diffs = database
            .get_songs()
            .await
            .expect("Failed to load songs from database");

        let mut difficulty_id_path_map: HashMap<u64, PathBuf> = HashMap::default();

        let mut all_songs: Vec<_> = diffs
            .drain(0..)
            .into_grouping_map_by(|x| x.folderid)
            .fold(Song::default(), |mut song, id, diff| {
                if song.difficulties.is_empty() {
                    song.id = *id as u64;
                    song.artist = diff.artist;
                    song.bpm = diff.bpm;
                    song.title = diff.title;
                }

                difficulty_id_path_map.insert(diff.rowid as u64, PathBuf::from(&diff.path));
                let diff_path = PathBuf::from(diff.path);
                song.difficulties.push(Difficulty {
                    jacket_path: diff_path.with_file_name(diff.jacket_path),
                    level: diff.level as u8,
                    difficulty: diff.diff_index as u8,
                    id: diff.rowid as u64,
                    effector: diff.effector,
                    top_badge: 0,           //TODO
                    scores: Vec::default(), //TODO
                    hash: Some(diff.hash),
                });

                song
            })
            .drain()
            .map(|(_, song)| Arc::new(song))
            .collect();

        for song in &mut all_songs {
            let song = Arc::make_mut(song);
            for diff in &mut song.difficulties {
                if let Ok(mut score_entires) = database
                    .get_scores_for_chart(diff.hash.as_ref().unwrap())
                    .await
                {
                    diff.scores = score_entires.drain(0..).map_into().collect();
                }
            }
        }

        FileSongProvider {
            new_songs: all_songs.clone(),
            all_songs,
            difficulty_id_path_map,
            events: Default::default(),
            database,
        }
    }

    fn load_songs_folder() {
        let song_path = crate::config::GameConfig::get().unwrap().songs_path.clone();
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
            HashMap::<PathBuf, Vec<(Chart, DirEntry)>>::new(),
            |mut acc, (dir, chart)| {
                if let Some(parent_folder) = dir.path().parent() {
                    acc.entry(parent_folder.to_path_buf())
                        .and_modify(|v| v.push((chart.clone(), dir.clone())))
                        .or_insert_with(|| vec![(chart, dir)]);
                }
                acc
            },
        );

        let _songs: Vec<Arc<Song>> = song_folders
            .into_iter()
            .enumerate()
            .map(|(id, (song_folder, charts))| {
                Arc::new(Song {
                    title: charts[0].0.meta.title.clone(),
                    artist: charts[0].0.meta.artist.clone(),
                    bpm: charts[0].0.meta.disp_bpm.clone(),
                    id: id as u64,
                    difficulties: charts
                        .iter()
                        .enumerate()
                        .map(|(id, c)| Difficulty {
                            top_badge: 0,
                            difficulty: c.0.meta.difficulty,
                            effector: c.0.meta.chart_author.clone(),
                            id: id as u64,
                            jacket_path: song_folder.join(&c.0.meta.jacket_filename),
                            level: c.0.meta.level,
                            scores: vec![Score::default()],
                            hash: Some("".into()),
                        })
                        .collect(),
                })
            })
            .collect();
    }
}

impl SongProvider for FileSongProvider {
    fn poll(&mut self) -> Option<super::SongProviderEvent> {
        if self.new_songs.is_empty() {
            self.events.pop_front()
        } else {
            let new_songs = std::mem::take(&mut self.new_songs);
            Some(super::SongProviderEvent::SongsAdded(new_songs))
        }
    }

    fn set_search(&mut self, _query: &str) {
        todo!()
    }

    fn set_sort(&mut self, _sort: super::SongSort) {
        todo!()
    }

    fn set_filter(&mut self, _filter: super::SongFilter) {
        todo!()
    }

    fn set_current_index(&mut self, _index: u64) {}

    fn load_song(
        &self,
        _index: u64,
        _diff_index: u64,
    ) -> Box<dyn FnOnce() -> (kson::Chart, Box<dyn rodio::Source<Item = f32> + Send>) + Send> {
        let path = self
            .difficulty_id_path_map
            .get(&_diff_index)
            .expect("No diff with that id")
            .clone();

        Box::new(move || {
            let chart = kson::Chart::from_ksh(
                &std::fs::read_to_string(&path).expect("Failed to read file"),
            )
            .expect("Failed to parse ksh");

            let audio = rodio::decoder::Decoder::new(
                std::fs::File::open(
                    path.with_file_name(
                        chart
                            .audio
                            .bgm
                            .as_ref()
                            .expect("Chart has no BGM info")
                            .filename
                            .as_ref()
                            .expect("Chart has no BGM filename"),
                    ),
                )
                .expect("Failed to open file"),
            )
            .expect("Failed to open chart audio");

            (chart, Box::new(audio.convert_samples()))
        })
    }
}

impl ScoreProvider for FileSongProvider {
    fn poll(&mut self) -> Option<super::ScoreProviderEvent> {
        todo!()
    }

    fn get_scores(&mut self, id: u64) -> Vec<Score> {
        todo!()
    }

    fn insert_score(&mut self, id: u64, score: Score) -> anyhow::Result<()> {
        todo!()
    }
}

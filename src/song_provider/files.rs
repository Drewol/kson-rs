use std::{
    collections::{HashMap, VecDeque},
    ops::Index,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
};

use crate::songselect::{Difficulty, Song};

use super::{SongProvider, SongProviderEvent};
use kson::{Chart, Ksh};
use log::info;

#[derive(Debug)]
pub struct FileSongProvider {
    all_songs: Vec<Arc<Song>>,
    new_songs: Vec<Arc<Song>>,
    difficulty_id_path_map: HashMap<u64, PathBuf>,
    events: VecDeque<SongProviderEvent>,
}

impl FileSongProvider {
    pub fn new() -> Self {
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

        let mut songs: Vec<Arc<Song>> = song_folders
            .into_iter()
            .enumerate()
            .map(|(id, (song_folder, charts))| {
                Arc::new(Song {
                    title: charts[0].meta.title.clone(),
                    artist: charts[0].meta.artist.clone(),
                    bpm: charts[0].meta.disp_bpm.clone(),
                    id: id as u64,
                    difficulties: charts
                        .iter()
                        .enumerate()
                        .map(|(id, c)| Difficulty {
                            best_badge: 0,
                            difficulty: c.meta.difficulty,
                            effector: c.meta.chart_author.clone(),
                            id: id as u64,
                            jacket_path: song_folder.join(&c.meta.jacket_filename),
                            level: c.meta.level,
                            scores: vec![99],
                        })
                        .collect(),
                })
            })
            .collect();

        songs.sort_by_key(|s| s.title.to_lowercase());

        FileSongProvider {
            all_songs: songs.clone(),
            new_songs: songs,
            difficulty_id_path_map: Default::default(),
            events: Default::default(),
        }
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

    fn set_search(&mut self, query: &str) {
        todo!()
    }

    fn set_sort(&mut self, sort: super::SongSort) {
        todo!()
    }

    fn set_filter(&mut self, filter: super::SongFilter) {
        todo!()
    }

    fn set_current_index(&mut self, index: u64) {
        todo!()
    }

    fn load_song(&mut self, index: u64) -> poll_promise::Promise<anyhow::Result<kson::Chart>> {
        todo!()
    }
}

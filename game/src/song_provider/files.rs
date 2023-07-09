use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    path::PathBuf,
    sync::{
        mpsc::{channel, Receiver, Sender, TryRecvError},
        Arc, Mutex, RwLock,
    },
    time::Duration,
};

use crate::{
    block_on,
    results::Score,
    songselect::{Difficulty, Song},
};

use super::{ScoreProvider, SongProvider, SongProviderEvent};
use anyhow::ensure;
use egui::epaint::ahash::HashSet;
use futures::{AsyncReadExt, StreamExt, TryStreamExt};
use itertools::Itertools;
use kson::{Chart, Ksh};
use log::info;
use puffin::profile_function;
use rodio::Source;
use rusc_database::{ChartEntry, LocalSongsDb, ScoreEntry};
use walkdir::DirEntry;

enum WorkerControlMessage {
    Stop,
    Refresh,
}

pub struct FileSongProvider {
    all_songs: Vec<Arc<Song>>,
    new_songs: Vec<Arc<Song>>,
    database: rusc_database::LocalSongsDb,
    worker: poll_promise::Promise<()>,
    worker_rx: Receiver<SongProviderEvent>,
    worker_tx: Sender<WorkerControlMessage>,
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

        let worker_db = database.clone();
        let (sender_tx, worker_rx) = channel();
        let (worker_tx, sender_rx) = channel();
        let worker = poll_promise::Promise::spawn_async(async move {
            loop {
                let mut songs = {
                    let song_path = crate::config::GameConfig::get().songs_path.clone();
                    info!("Loading songs from: {:?}", &song_path);
                    let song_walker = walkdir::WalkDir::new(song_path);

                    song_walker
                        .into_iter()
                        .filter_map(|a| a.ok())
                        .filter(|e| {
                            e.file_type().is_file()
                                && matches!(
                                    e.path().extension().and_then(|x| x.to_str()),
                                    Some("ksh" | "kson")
                                )
                        })
                        .filter_map(|e| e.path().parent().map(|x| x.to_path_buf()))
                        .unique()
                        .chunks(1)
                        .into_iter()
                        .map(|folders| {
                            let worker_db = worker_db.clone();
                            let sender_tx = sender_tx.clone();
                            let folders = folders.collect_vec();
                            async move {
                                let mut songs = vec![];
                                for folder in folders {
                                    match worker_db.get_or_insert_folder(&folder).await {
                                        Ok(folder_id) => {
                                            let mut charts = vec![];
                                            match async_fs::read_dir(&folder).await {
                                                Ok(mut dir) => {
                                                    while let Some(f) = dir.next().await {
                                                        if let Some(err) = f.as_ref().err() {
                                                            log::warn!("{}", err);
                                                            continue;
                                                        }

                                                        let f = f.unwrap();
                                                        if !f.path().is_file() {
                                                            continue;
                                                        }
                                                        let ext = f
                                                            .path()
                                                            .extension()
                                                            .and_then(|x| x.to_str())
                                                            .map(|x| x.to_lowercase());
                                                        if let Some(ext) = ext {
                                                            if ext != "ksh" {
                                                                //TODO: kson
                                                                continue;
                                                            }
                                                        } else {
                                                            continue;
                                                        }

                                                        let mut hasher: sha1_smol::Sha1 =
                                                            sha1_smol::Sha1::new();

                                                        let mut data = vec![];

                                                        if let Ok(mut p) =
                                                            async_fs::File::open(&f.path()).await
                                                        {
                                                            _ = p.read_to_end(&mut data).await;
                                                        }
                                                        hasher.update(&data);
                                                        let hash = hasher.digest().to_string();

                                                        if worker_db
                                                            .get_hash_id(&hash)
                                                            .await
                                                            .is_ok_and(|x| x.is_some())
                                                        {
                                                            continue; //Already added
                                                        }

                                                        let (c, _) = encoding::types::decode(
                                                            &data,
                                                            encoding::DecoderTrap::Strict,
                                                            encoding::all::WINDOWS_31J,
                                                        );
                                                        if let Some(err) = c.as_ref().err() {
                                                            log::warn!("{:?}: {}", f.path(), err);
                                                            continue;
                                                        }

                                                        let c = kson::Chart::from_ksh(&c.unwrap());

                                                        if let Some(err) = c.as_ref().err() {
                                                            log::warn!("{:?}: {}", f.path(), err);
                                                            continue;
                                                        }

                                                        let c = c.unwrap();

                                                        if c.get_last_tick() > 0 {
                                                            charts.push((f.path(), c, hash));
                                                        }
                                                    }
                                                }
                                                Err(e) => log::warn!("{e}"),
                                            }
                                            let mut song = Song {
                                                title: String::new(),
                                                artist: String::new(),
                                                bpm: String::new(),
                                                id: folder_id as _,
                                                difficulties: vec![],
                                            };
                                            for (path, c, hash) in charts {
                                                let scores = worker_db
                                                    .get_scores_for_chart(&hash)
                                                    .await
                                                    .map(|mut x| {
                                                        x.drain(..).map(Score::from).collect_vec()
                                                    })
                                                    .unwrap_or_default();
                                                let jacket_path = path
                                                    .with_file_name(c.meta.jacket_filename.clone());
                                                let id = if let Ok(Some(id)) =
                                                    worker_db.get_hash_id(&hash).await
                                                {
                                                    id as u64
                                                } else {
                                                    worker_db
                                                        .add_chart(ChartEntry {
                                                            rowid: 0,
                                                            folderid: folder_id,
                                                            path: path
                                                                .to_string_lossy()
                                                                .to_string(),
                                                            title: c.meta.title.clone(),
                                                            artist: c.meta.artist.clone(),
                                                            title_translit: String::new(),
                                                            artist_translit: String::new(),
                                                            jacket_path: jacket_path
                                                                .to_string_lossy()
                                                                .to_string(),
                                                            effector: c.meta.chart_author.clone(),
                                                            illustrator: c
                                                                .meta
                                                                .jacket_author
                                                                .clone(),
                                                            diff_name: String::new(),
                                                            diff_shortname: String::new(),
                                                            bpm: c.meta.disp_bpm.clone(),
                                                            diff_index: c.meta.difficulty as _,
                                                            level: c.meta.level as _,
                                                            hash: hash.to_string(),
                                                            preview_file: Some(
                                                                path.with_file_name(
                                                                    c.audio
                                                                        .bgm
                                                                        .as_ref()
                                                                        .unwrap()
                                                                        .filename
                                                                        .clone()
                                                                        .unwrap(),
                                                                )
                                                                .to_string_lossy()
                                                                .to_string(),
                                                            ),
                                                            preview_offset: c
                                                                .audio
                                                                .bgm
                                                                .as_ref()
                                                                .unwrap()
                                                                .preview
                                                                .offset
                                                                as _,
                                                            preview_length: c
                                                                .audio
                                                                .bgm
                                                                .as_ref()
                                                                .unwrap()
                                                                .preview
                                                                .duration
                                                                as _,
                                                            lwt: std::fs::metadata(path)
                                                                .and_then(|x| x.modified())
                                                                .map(|x| {
                                                                    x.elapsed().unwrap_or_default()
                                                                })
                                                                .map(|x| x.as_secs())
                                                                .unwrap_or_default()
                                                                as _,
                                                            custom_offset: 0,
                                                        })
                                                        .await
                                                        .expect("Failed to insert chart")
                                                        as u64
                                                };

                                                song.bpm = c.meta.disp_bpm.clone();
                                                song.title = c.meta.title.clone();
                                                song.artist = c.meta.artist.clone();

                                                song.difficulties.push(Difficulty {
                                                    jacket_path,
                                                    level: c.meta.level,
                                                    difficulty: c.meta.difficulty,
                                                    id,
                                                    effector: c.meta.chart_author.clone(),
                                                    top_badge: scores
                                                        .iter()
                                                        .map(|x| x.badge)
                                                        .max()
                                                        .unwrap_or_default(),
                                                    scores,
                                                    hash: Some(hash),
                                                });
                                            }

                                            if song.difficulties.is_empty() {
                                                return;
                                            }

                                            //TODO: not this
                                            info!(
                                                "Loaded song: {} - {}",
                                                &song.title, &song.artist
                                            );

                                            songs.push(Arc::new(song));
                                        }
                                        Err(e) => log::error!("{}", e),
                                    }
                                }

                                if sender_tx
                                    .send(SongProviderEvent::SongsRemoved(
                                        songs.iter().map(|x| x.id).collect(),
                                    ))
                                    .is_err()
                                {
                                    return;
                                }
                                if sender_tx
                                    .send(SongProviderEvent::SongsAdded(songs))
                                    .is_err()
                                {}
                            }
                        })
                }
                .collect_vec();

                futures::future::join_all(songs).await;

                //send update to provider
                info!("Finished importing");
                loop {
                    match sender_rx.try_recv() {
                        Ok(WorkerControlMessage::Refresh) => break,
                        Ok(WorkerControlMessage::Stop) => return,
                        Err(TryRecvError::Disconnected) => return,
                        Err(TryRecvError::Empty) => {}
                    }

                    futures::pending!() //yield async task
                }
            }
        });

        FileSongProvider {
            new_songs: all_songs.clone(),
            all_songs,
            database,
            worker,
            worker_rx,
            worker_tx,
        }
    }
}

impl SongProvider for FileSongProvider {
    fn poll(&mut self) -> Option<super::SongProviderEvent> {
        if self.new_songs.is_empty() {
            self.worker
                .ready()
                .is_some()
                .then(|| panic!("Song file provider worker returned")); //panics if worker paniced
            self.worker_rx.try_recv().ok()
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
        let db = self.database.clone();
        let path = PathBuf::from(
            block_on!(db.get_song(_diff_index as _))
                .expect("No diff with that id")
                .path,
        );

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

    fn get_preview(
        &self,
        id: u64,
    ) -> anyhow::Result<(
        Box<dyn Source<Item = f32> + Send>,
        std::time::Duration,
        std::time::Duration,
    )> {
        profile_function!();
        let id = id as i64;

        let db = self.database.clone();
        let mut charts = block_on!(db.get_charts_for_folder(id))?;
        let chart = charts.pop();

        ensure!(chart.is_some());
        let mut chart = chart.unwrap();

        info!("Got chart: {:?}", &chart.preview_file);
        ensure!(chart.preview_file.is_some());
        let path = chart.preview_file.take().unwrap();

        let source = rodio::Decoder::new(std::fs::File::open(
            PathBuf::from(&chart.path).with_file_name(path),
        )?)?
        .convert_samples();
        Ok((
            Box::new(source),
            Duration::from_millis(chart.preview_offset as u64),
            Duration::from_millis(chart.preview_length as u64),
        ))
    }
}

impl ScoreProvider for FileSongProvider {
    fn poll(&mut self) -> Option<super::ScoreProviderEvent> {
        todo!()
    }

    fn get_scores(&mut self, _id: u64) -> Vec<Score> {
        todo!()
    }

    fn insert_score(&mut self, _id: u64, _score: Score) -> anyhow::Result<()> {
        todo!()
    }
}

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        mpsc::{channel, Receiver, Sender, TryRecvError},
        Arc, RwLock,
    },
    time::Duration,
};

use crate::{
    block_on,
    config::{GameConfig, SongSelectSettings},
    game::HitWindow,
    results::Score,
    song_provider::SongFilterType,
    songselect::{Difficulty, Song},
    worker_service::WorkerService,
};

use super::{
    DiffId, LoadSongFn, ScoreProvider, ScoreProviderEvent, SongDiffId, SongFilter, SongId,
    SongProvider, SongProviderEvent, SongSort,
};
use anyhow::{anyhow, bail, ensure};

use futures::{executor::block_on, AsyncReadExt, StreamExt};
use itertools::Itertools;
use kson::Ksh;
use log::{info, warn};
use puffin::profile_function;
use rodio::Source;
use rusc_database::{ChartEntry, LocalSongsDb, ScoreEntry};
use tokio::io::AsyncRead;

enum WorkerControlMessage {
    Stop,
    Refresh,
    Query(String, SongFilter, SongSort),
}

pub struct FileSongProvider {
    all_songs: Vec<Arc<Song>>,

    database: rusc_database::LocalSongsDb,
    worker: poll_promise::Promise<()>,
    worker_rx: Receiver<SongProviderEvent>,
    worker_tx: Sender<WorkerControlMessage>,
    score_bus: bus::Bus<ScoreProviderEvent>,
    song_bus: bus::Bus<SongProviderEvent>,
    sort: SongSort,
    filter: SongFilter,
    query: String,
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
            hit_window: HitWindow::new(
                0,
                value.window_perfect as _,
                value.window_good as _,
                value.window_hold as _,
                value.window_miss as _,
            ),
            earlies: value.early as _,
            lates: value.late as _,
            combo: value.combo as _,
        }
    }
}

impl FileSongProvider {
    pub async fn new() -> Self {
        let mut db_file = GameConfig::get().game_folder.clone();
        db_file.push("maps.db");

        let database = LocalSongsDb::new(db_file)
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
                //This fold clones the initial state, and with our difficulties being RCd, we need to reinit the diffs
                if song.id == SongId::Missing {
                    song.id = SongId::IntId(*id);
                    song.artist = diff.artist;
                    song.bpm = diff.bpm;
                    song.title = diff.title;
                    song.difficulties = Arc::new(RwLock::new(vec![]));
                }
                let mut difficulties = song.difficulties.write().expect("Lock error");

                difficulty_id_path_map.insert(diff.rowid as u64, PathBuf::from(&diff.path));
                let diff_path = PathBuf::from(diff.path);
                difficulties.push(Difficulty {
                    jacket_path: diff_path.with_file_name(diff.jacket_path),
                    level: diff.level as u8,
                    difficulty: diff.diff_index as u8,
                    id: DiffId(SongId::StringId(diff.hash.clone())),
                    effector: diff.effector,
                    top_badge: 0,           //TODO
                    scores: Vec::default(), //TODO
                    hash: Some(diff.hash),
                });
                drop(difficulties);
                song
            })
            .drain()
            .map(|(_, song)| Arc::new(song))
            .collect();

        let worker_db = database.clone();
        let (sender_tx, worker_rx) = channel();
        let (worker_tx, sender_rx) = channel(); //TODO: Async channels?
        let worker = poll_promise::Promise::spawn_async(async move {
            loop {
                let songs = {
                    let song_path = songs_path();
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
                                            match tokio::fs::read_dir(&folder).await {
                                                Ok(mut dir) => {
                                                    while let Ok(Some(f)) = dir.next_entry().await {
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

                                                        let Ok(data) =
                                                            tokio::fs::read(f.path()).await
                                                        else {
                                                            continue;
                                                        };

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
                                                        let c = match c {
                                                            Ok(c) => c,
                                                            Err(err) => {
                                                                log::warn!(
                                                                    "{:?}: {}",
                                                                    f.path(),
                                                                    err
                                                                );

                                                                continue;
                                                            }
                                                        };

                                                        let c = kson::Chart::from_ksh(&c);

                                                        let c = match c {
                                                            Ok(c) => c,
                                                            Err(err) => {
                                                                log::warn!(
                                                                    "{:?}: {}",
                                                                    f.path(),
                                                                    err
                                                                );

                                                                continue;
                                                            }
                                                        };
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
                                                id: SongId::IntId(folder_id),
                                                difficulties: Arc::new(vec![].into()),
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
                                                    id
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
                                                                    c.audio.bgm.filename.clone(),
                                                                )
                                                                .to_string_lossy()
                                                                .to_string(),
                                                            ),
                                                            preview_offset: c
                                                                .audio
                                                                .bgm
                                                                .preview
                                                                .offset
                                                                as _,
                                                            preview_length: c
                                                                .audio
                                                                .bgm
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
                                                };

                                                song.bpm.clone_from(&c.meta.disp_bpm);
                                                song.title.clone_from(&c.meta.title);
                                                song.artist.clone_from(&c.meta.artist);

                                                song.difficulties
                                                    .write()
                                                    .expect("Lock error")
                                                    .push(Difficulty {
                                                        jacket_path,
                                                        level: c.meta.level,
                                                        difficulty: c.meta.difficulty,
                                                        id: DiffId(SongId::StringId(hash.clone())),
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

                                            if song
                                                .difficulties
                                                .read()
                                                .expect("Lock error")
                                                .is_empty()
                                            {
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
                                        songs.iter().map(|x| x.id.clone()).collect(),
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
                        Ok(WorkerControlMessage::Query(q, filter, sort)) => {
                            let folder = if let SongFilterType::Folder(folder) = filter.filter_type
                            {
                                let mut p = songs_path();
                                p.push(folder);
                                Some(p.to_string_lossy().to_string())
                            } else {
                                None
                            };
                            let charts = match worker_db
                                .get_folder_ids_query(&q, filter.level, folder, sort.into())
                                .await
                            {
                                Ok(charts) => charts,
                                Err(e) => {
                                    warn!("{e}");
                                    continue;
                                }
                            };

                            sender_tx.send(SongProviderEvent::OrderChanged(
                                charts.iter().map(|x| SongId::IntId(*x)).collect(),
                            ));

                            //TODO
                        }
                        Ok(WorkerControlMessage::Stop) => return,
                        Err(TryRecvError::Disconnected) => return,
                        Err(TryRecvError::Empty) => {}
                    }

                    tokio::time::sleep(Duration::from_millis(100)).await
                }
            }
        });

        let SongSelectSettings { sorting, filter } = &GameConfig::get().song_select;

        FileSongProvider {
            all_songs,
            database,
            worker,
            worker_rx,
            score_bus: bus::Bus::new(32),
            song_bus: bus::Bus::new(32),
            worker_tx,
            sort: *sorting,
            filter: filter.clone(),
            query: String::new(),
        }
    }
}

fn songs_path() -> PathBuf {
    let song_path = crate::config::GameConfig::get().songs_path.clone();

    if song_path.is_absolute() {
        song_path
    } else {
        let mut p = GameConfig::get().game_folder.clone();
        p.push(song_path);
        p
    }
}

impl WorkerService for FileSongProvider {
    fn update(&mut self) {
        self.worker
            .ready()
            .is_some()
            .then(|| panic!("Song file provider worker returned")); //panics if worker paniced
        let ev = self.worker_rx.try_recv().ok();

        if let Some(ev) = ev {
            match &ev {
                SongProviderEvent::SongsAdded(s) => self.all_songs.append(&mut s.clone()),
                SongProviderEvent::SongsRemoved(r) => self.all_songs.retain(|s| !r.contains(&s.id)),
                SongProviderEvent::OrderChanged(_) => {}
            }

            self.song_bus.broadcast(ev);
        }
    }
}

impl SongProvider for FileSongProvider {
    fn set_search(&mut self, q: &str) {
        self.query = q.to_string();
        self.worker_tx.send(WorkerControlMessage::Query(
            q.to_string(),
            self.filter.clone(),
            self.sort,
        ));
    }

    fn set_sort(&mut self, sort: super::SongSort) {
        self.sort = sort;
        self.worker_tx.send(WorkerControlMessage::Query(
            self.query.clone(),
            self.filter.clone(),
            self.sort,
        ));
        GameConfig::get_mut().song_select.sorting = self.sort;
    }

    fn set_filter(&mut self, filter: super::SongFilter) {
        self.filter = filter;
        self.worker_tx.send(WorkerControlMessage::Query(
            self.query.clone(),
            self.filter.clone(),
            self.sort,
        ));
        GameConfig::get_mut().song_select.filter = self.filter.clone();
    }

    fn set_current_index(&mut self, _index: u64) {}

    fn load_song(&self, id: &SongDiffId) -> anyhow::Result<LoadSongFn> {
        let _diff_index = match id {
            SongDiffId::DiffOnly(diff_id) | SongDiffId::SongDiff(_, diff_id) => match &diff_id.0 {
                SongId::IntId(id) => *id,
                SongId::StringId(hash) => {
                    block_on(self.database.get_hash_id(hash))?.ok_or(anyhow!("No song hash"))?
                }
                SongId::Missing => todo!(),
            },
            _ => todo!(),
        };

        let db = self.database.clone();
        let path = PathBuf::from(block_on!(db.get_song(_diff_index as _))?.path);

        Ok(Box::new(move || {
            let data = std::fs::read(&path)?;
            let data = encoding::decode(
                &data,
                encoding::DecoderTrap::Strict,
                encoding::all::WINDOWS_31J,
            )
            .0
            .map_err(|_| anyhow!("Bad encodiing"))?;

            let chart = kson::Chart::from_ksh(&data)?;

            let audio = rodio::decoder::Decoder::new(std::fs::File::open(
                path.with_file_name(&chart.audio.bgm.filename),
            )?)?;

            Ok((chart, Box::new(audio.convert_samples())))
        }))
    }

    fn get_preview(
        &self,
        id: &SongId,
    ) -> anyhow::Result<(
        Box<dyn Source<Item = f32> + Send>,
        std::time::Duration,
        std::time::Duration,
    )> {
        profile_function!();
        let SongId::IntId(id) = id else {
            bail!("Unsupported id type")
        };
        let id = *id;
        let db = self.database.clone();
        let mut charts = block_on!(db.get_charts_for_folder(id))?;
        let Some(mut chart) = charts.pop() else {
            bail!("No chart found")
        };

        info!("Got chart: {:?}", &chart.preview_file);
        let Some(path) = chart.preview_file.take() else {
            bail!("No preview file")
        };

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

    fn subscribe(&mut self) -> bus::BusReader<SongProviderEvent> {
        self.song_bus.add_rx()
    }

    fn get_all(&self) -> Vec<Arc<Song>> {
        //TODO: a bit ugly but trigger query here to initialize the sort array as well
        self.worker_tx.send(WorkerControlMessage::Query(
            self.query.clone(),
            self.filter.clone(),
            self.sort,
        ));
        self.all_songs.clone()
    }

    fn add_score(&self, id: SongDiffId, score: Score) {
        let song = match &id {
            SongDiffId::Missing => None,
            SongDiffId::DiffOnly(diff) => self.all_songs.iter().find(|x| {
                x.difficulties
                    .read()
                    .expect("Lock error")
                    .iter()
                    .any(|d| d.id == *diff)
            }),
            SongDiffId::SongDiff(song, diff) => self.all_songs.iter().find(|x| x.id == *song),
        };

        if let (Some(song), Some(diff)) = (song, id.get_diff()) {
            let diffs = &mut song.difficulties.write().expect("Lock error");
            let diff = diffs.iter_mut().find(|x| x.id == *diff);
            if let Some(diff) = diff {
                diff.top_badge = diff.top_badge.max(score.badge);
                diff.scores.push(score);
                diff.scores.sort_by_key(|x| -x.score);
            }
        }
    }

    fn get_available_sorts(&self) -> Vec<super::SongSort> {
        vec![
            super::SongSort::new(
                crate::song_provider::SongSortType::Title,
                crate::song_provider::SortDir::Desc,
            ),
            super::SongSort::new(
                crate::song_provider::SongSortType::Title,
                crate::song_provider::SortDir::Asc,
            ),
            super::SongSort::new(
                crate::song_provider::SongSortType::Score,
                crate::song_provider::SortDir::Asc,
            ),
            super::SongSort::new(
                crate::song_provider::SongSortType::Score,
                crate::song_provider::SortDir::Desc,
            ),
            super::SongSort::new(
                crate::song_provider::SongSortType::Date,
                crate::song_provider::SortDir::Asc,
            ),
            super::SongSort::new(
                crate::song_provider::SongSortType::Date,
                crate::song_provider::SortDir::Desc,
            ),
            super::SongSort::new(
                crate::song_provider::SongSortType::Artist,
                crate::song_provider::SortDir::Asc,
            ),
            super::SongSort::new(
                crate::song_provider::SongSortType::Artist,
                crate::song_provider::SortDir::Desc,
            ),
            super::SongSort::new(
                crate::song_provider::SongSortType::Effector,
                crate::song_provider::SortDir::Asc,
            ),
            super::SongSort::new(
                crate::song_provider::SongSortType::Effector,
                crate::song_provider::SortDir::Desc,
            ),
        ]
    }

    fn get_available_filters(&self) -> Vec<super::SongFilterType> {
        let songs_path = songs_path();
        let Ok(song_path_contents): Result<Vec<_>, _> =
            songs_path.read_dir().and_then(|x| x.try_collect())
        else {
            log::warn!("Failed to iterate folders");
            return vec![];
        };

        let mut res = vec![super::SongFilterType::None];

        res.extend(
            song_path_contents
                .into_iter()
                .filter(|x| x.path().is_dir())
                .filter(|x| {
                    //Read subdirs and check for .ksh files in the top folder
                    x.path().read_dir().is_ok_and(|mut x| {
                        !x.any(|x| {
                            x.is_ok_and(|x| {
                                x.path()
                                    .extension()
                                    .and_then(|f| f.to_str())
                                    .is_some_and(|f| f.to_lowercase() == "ksh")
                            })
                        })
                    })
                })
                .map(|x| {
                    super::SongFilterType::Folder(x.file_name().to_string_lossy().to_string())
                }),
        );
        res
    }
}

impl ScoreProvider for FileSongProvider {
    fn get_scores(&mut self, _id: &SongDiffId) -> Vec<Score> {
        todo!()
    }

    fn insert_score(&mut self, id: &SongDiffId, score: Score) -> anyhow::Result<()> {
        {
            let Score {
                gauge,
                gauge_type,
                gauge_option,
                mirror,
                random,
                auto_flags,
                score,
                perfects,
                goods,
                misses,
                badge,
                timestamp,
                is_local,
                hit_window,
                earlies,
                lates,
                combo,
                ..
            } = score;

            let Some(DiffId(SongId::StringId(hash))) = id.get_diff() else {
                bail!("Hash required")
            };

            block_on(self.database.add_score(ScoreEntry {
                rowid: 0,
                score: score as _,
                crit: perfects as _,
                near: goods as _,
                early: earlies as _,
                late: lates as _,
                combo: combo as _,
                miss: misses as _,
                gauge: gauge as _,
                auto_flags: auto_flags as _,
                replay: None,
                timestamp: timestamp as _,
                chart_hash: hash.to_string(),
                user_name: "".to_string(),
                user_id: "".to_string(),
                local_score: true,
                window_perfect: hit_window.perfect.as_millis() as _,
                window_good: hit_window.good.as_millis() as _,
                window_hold: hit_window.hold.as_millis() as _,
                window_miss: hit_window.miss.as_millis() as _,
                window_slam: hit_window.good.as_millis() as _,
                gauge_type: 0,
                gauge_opt: 0,
                mirror,
                random,
            }))?;
        }

        self.score_bus
            .broadcast(ScoreProviderEvent::NewScore(id.clone(), score));

        Ok(())
    }

    fn subscribe(&mut self) -> bus::BusReader<ScoreProviderEvent> {
        self.score_bus.add_rx()
    }

    fn init_scores(&self, songs: &mut dyn Iterator<Item = &Arc<Song>>) -> anyhow::Result<()> {
        let mut scores = block_on(self.database.get_all_scores())?;

        let mut scores = scores
            .drain(..)
            .group_by(|x| DiffId(SongId::StringId(x.chart_hash.clone()))) //TODO: Excessive cloning
            .into_iter()
            .map(|(key, scores)| (key, scores.map(Score::from).collect_vec()))
            .collect::<HashMap<_, _>>();

        songs.for_each(|song| {
            let mut diffs = song.difficulties.write().expect("Lock error");
            for diff in diffs.iter_mut() {
                diff.scores = scores.remove(&diff.id).unwrap_or_default();
                diff.scores.sort_by_key(|x| -x.score);
                diff.top_badge = diff
                    .scores
                    .iter()
                    .map(|x| x.badge)
                    .max()
                    .unwrap_or_default();
            }

            diffs.sort_by_key(|x| (x.difficulty, x.level))
        });

        Ok(())
    }
}

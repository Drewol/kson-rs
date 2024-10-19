use std::{
    collections::{HashMap, VecDeque},
    fmt::Debug,
    fs::File,
    io::{BufReader, BufWriter, Read},
    path::PathBuf,
    str::FromStr,
    sync::{mpsc::Sender, Arc},
    time::Duration,
};

use di::RefMut;
use futures::AsyncWriteExt;
use itertools::Itertools;
use log::warn;
use rodio::Source;

use crate::{
    async_service::AsyncService,
    project_dirs,
    results::Score,
    song_provider::SongFilterType,
    songselect::{Difficulty, Song},
    worker_service::WorkerService,
};

use super::{DiffId, LoadSongFn, SongDiffId, SongFilter, SongId, SongProvider, SongProviderEvent};
use anyhow::{anyhow, bail, ensure, Result};
use kson::Ksh;
use poll_promise::Promise;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub struct NauticaSongs {
    pub(crate) data: Vec<Datum>,
    pub(crate) links: Links,
    pub(crate) meta: Meta,
}

#[derive(Default, Serialize, Deserialize)]
struct LocalData {
    songs: HashMap<Uuid, Datum>,
}

#[derive(Serialize, Deserialize)]
pub struct NauticaSong {
    pub(crate) data: Datum,
}

#[derive(Serialize, Deserialize)]
pub struct Datum {
    pub(crate) id: Uuid,
    pub(crate) user_id: Uuid,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) jacket_filename: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) download_link: Option<String>,
    pub(crate) downloads: i64,
    pub(crate) has_preview: i64,
    pub(crate) hidden: i64,
    pub(crate) mojibake: i64,
    pub(crate) uploaded_at: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) jacket_url: String,
    pub(crate) preview_url: Option<String>,
    pub(crate) cdn_download_url: String,
    pub(crate) user: User,
    pub(crate) charts: Vec<Chart>,
    pub(crate) tags: Vec<Tag>,
}

#[derive(Serialize, Deserialize)]
pub struct Chart {
    pub(crate) id: Uuid,
    pub(crate) user_id: Uuid,
    pub(crate) song_id: Uuid,
    pub(crate) difficulty: i64,
    pub(crate) level: i64,
    pub(crate) effector: String,
    pub(crate) video_link: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Serialize, Deserialize)]
pub struct Tag {
    pub(crate) id: Uuid,
    pub(crate) song_id: Uuid,
    pub(crate) value: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Serialize, Deserialize)]
pub struct User {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    #[serde(rename = "urlRoute")]
    pub(crate) url_route: String,
    pub(crate) twitter: Option<String>,
    pub(crate) youtube: Option<String>,
    pub(crate) bio: Option<String>,
    pub(crate) created_at: String,
    #[serde(rename = "songCount")]
    pub(crate) song_count: i64,
}

#[derive(Serialize, Deserialize)]
pub struct Links {
    pub(crate) first: String,
    pub(crate) last: String,
    pub(crate) prev: Option<String>,
    pub(crate) next: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Meta {
    pub(crate) current_page: i64,
    pub(crate) from: i64,
    pub(crate) last_page: i64,
    pub(crate) path: String,
    pub(crate) per_page: i64,
    pub(crate) to: i64,
    pub(crate) total: i64,
}

impl Datum {
    fn as_song(&self) -> Song {
        let Datum {
            id,
            user_id: _,
            title,
            artist,
            jacket_filename: _,
            description: _,
            download_link: _,
            downloads: _,
            has_preview: _,
            hidden: _,
            mojibake: _,
            uploaded_at: _,
            created_at: _,
            updated_at: _,
            jacket_url,
            preview_url: _,
            cdn_download_url: _,
            user: _,
            charts,
            tags: _,
        } = self;

        let mut song_path = project_dirs().cache_dir().to_path_buf();
        song_path.push(id.as_hyphenated().to_string());

        std::fs::create_dir_all(&song_path);
        song_path.push("jacket.png");
        let jacket_path = if jacket_url.ends_with("png") {
            song_path
        } else {
            song_path.with_extension("jpg")
        };

        Song {
            title: title.clone(),
            artist: artist.clone(),
            bpm: "unk".to_string(),
            id: SongId::StringId(id.as_hyphenated().to_string()),
            difficulties: Arc::new(
                charts
                    .iter()
                    .map(|x| x.as_diff(jacket_path.clone()))
                    .collect_vec()
                    .into(),
            ),
        }
    }
}

impl Chart {
    fn as_diff(&self, jacket_path: PathBuf) -> Difficulty {
        let Chart {
            id: uid,
            user_id: _,
            song_id: _,
            difficulty,
            level,
            effector,
            video_link: _,
            created_at: _,
            updated_at: _,
        } = self;

        Difficulty {
            jacket_path,
            level: *level as u8,
            difficulty: *difficulty as u8 - 1,
            id: DiffId(SongId::StringId(uid.as_hyphenated().to_string())),
            effector: effector.clone(),
            top_badge: 0,
            scores: vec![],
            hash: None,
            illustrator: String::new(),
        }
    }
}

pub struct NauticaSongProvider {
    next: Option<Promise<Result<NauticaSongs>>>,
    events: VecDeque<SongProviderEvent>,
    all_songs: Vec<Arc<Song>>,
    next_url: String,
    bus: bus::Bus<SongProviderEvent>,
    filter: SongFilter,
    query: HashMap<&'static str, String>,
    local_data: LocalData,
    song_loaded: (
        std::sync::mpsc::Sender<Datum>,
        std::sync::mpsc::Receiver<Datum>,
    ),
    async_worker: Arc<std::sync::RwLock<AsyncService>>,
}

impl Debug for NauticaSongProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NauticaSongProvider").finish()
    }
}

async fn next_songs(path: String) -> Result<NauticaSongs> {
    log::info!("Getting more nautica songs: {}", path);
    //TODO: Async requests
    let nautica_songs = reqwest::get(&path).await?.json::<NauticaSongs>().await?;
    for x in &nautica_songs.data {
        let mut song_path = project_dirs().cache_dir().to_path_buf();
        song_path.push(x.id.hyphenated().to_string());
        std::fs::create_dir_all(&song_path)?;
        if x.jacket_url.ends_with("png") {
            song_path.push("jacket.png");
        } else {
            song_path.push("jacket.jpg");
        }

        if song_path.exists() {
            continue;
        }

        let Ok(jacket_response) = reqwest::get(&x.jacket_url).await else {
            continue;
        };

        let jacket_path = match jacket_response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .ok_or(anyhow!("No content type header"))?
            .to_str()
            .unwrap_or_default()
        {
            "image/jpeg" => song_path.with_extension("jpg"),
            "image/png" => song_path.with_extension("png"),
            "image/webp" => song_path.with_extension("webp"),
            content_type => {
                warn!("Can't load jackets of type: {content_type}");
                continue;
            }
        };

        let Ok(bytes) = jacket_response.bytes().await else {
            continue;
        };

        tokio::fs::write(jacket_path, bytes).await;
    }
    Ok(nautica_songs)
}

impl NauticaSongProvider {
    pub fn new(async_worker: RefMut<AsyncService>) -> Self {
        let local_data = std::fs::read_to_string(cache_path())
            .ok()
            .and_then(|x| serde_json::from_str(&x).ok())
            .unwrap_or_default();

        Self {
            next: None,
            events: VecDeque::new(),
            all_songs: vec![],
            next_url: "https://ksm.dev/app/songs".into(),
            bus: bus::Bus::new(32),
            filter: SongFilter::new(SongFilterType::None, 0),
            query: HashMap::new(),
            local_data,
            song_loaded: std::sync::mpsc::channel(),
            async_worker,
        }
    }

    fn query_changed(&mut self) {
        let old_songs = std::mem::take(&mut self.all_songs);
        self.events.push_back(SongProviderEvent::SongsRemoved(
            old_songs.into_iter().map(|x| x.id.clone()).collect(),
        ));
        if let SongFilterType::Collection(c) = &self.filter.filter_type {
            self.all_songs = self
                .local_data
                .songs
                .iter()
                .map(|x| Arc::new(x.1.as_song()))
                .collect();

            self.events
                .push_back(SongProviderEvent::SongsAdded(self.all_songs.clone()));
        } else {
            let query = self
                .query
                .iter()
                .map(|x| format!("{}={}", x.0, x.1))
                .join("&");
            self.next_url = if query.is_empty() {
                "https://ksm.dev/app/songs".to_owned()
            } else {
                format!("https://ksm.dev/app/songs?{}", query)
            };
            self.next = Some(Promise::spawn_async(next_songs(self.next_url.clone())));
        }
    }
}

impl WorkerService for NauticaSongProvider {
    fn update(&mut self) {
        if let Some(next) = self.next.take() {
            match next.try_take() {
                Ok(Ok(songs)) => {
                    let new_songs = songs
                        .data
                        .iter()
                        .map(|d| Arc::new(d.as_song()))
                        .collect_vec();

                    self.all_songs.append(&mut new_songs.clone());
                    self.next_url = songs.links.next.unwrap_or_default();
                    self.events
                        .push_back(SongProviderEvent::SongsAdded(new_songs));
                }
                Ok(Err(e)) => log::error!("{}", e),
                Err(next) => self.next = Some(next),
            }
        } else {
            //TODO: Check scroll position and request more songs
        }

        if self.bus.rx_count() > 0 {
            for ele in self.events.drain(..) {
                self.bus.broadcast(ele);
            }
        }

        if let Ok(loaded) = self.song_loaded.1.try_recv() {
            self.local_data.songs.insert(loaded.id, loaded);

            if let Ok(local_data_json) = serde_json::to_string(&self.local_data) {
                self.async_worker.read().unwrap().run(async move {
                    use tokio::io::*;
                    let path = cache_path();
                    let Ok(mut file) = tokio::fs::File::create(&path).await else {
                        warn!("Could not create nautica cache file");
                        return;
                    };

                    if let Some(e) = file.write_all(local_data_json.as_bytes()).await.err() {
                        warn!("Could not write nautica cache file: {e}");
                    }
                })
            }
        }
    }
}

fn cache_path() -> PathBuf {
    let mut path = project_dirs().cache_dir().to_path_buf();
    path.push("nautica_cache.json");
    path
}

impl SongProvider for NauticaSongProvider {
    fn get_available_filters(&self) -> Vec<super::SongFilterType> {
        vec![
            SongFilterType::None,
            SongFilterType::Collection("Played".into()),
        ]
    }

    fn set_search(&mut self, query: &str) {
        if query.is_empty() {
            self.query.remove("q");
        } else {
            self.query.insert("q", query.to_owned());
        }
        self.query_changed();
    }

    fn set_sort(&mut self, _sort: super::SongSort) {
        todo!()
    }

    fn set_filter(&mut self, filter: super::SongFilter) {
        if filter.level > 0 {
            self.query.insert("levels", filter.level.to_string());
        } else {
            self.query.remove("levels");
        }
        self.filter = filter;

        self.query_changed();
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

    fn set_current_index(&mut self, index: u64) {
        if self.next.is_some() {
            return;
        }

        if let Some((i, _)) = self
            .all_songs
            .iter()
            .enumerate()
            .find(|x| x.1.id.as_u64() == index)
        {
            if i > self.all_songs.len().saturating_sub(10) {
                self.next = Some(Promise::spawn_async(next_songs(self.next_url.clone())));
            }
        }
    }

    fn load_song(&self, id: &SongDiffId) -> anyhow::Result<LoadSongFn> {
        let SongDiffId::SongDiff(SongId::StringId(song_id), diff_id) = id else {
            bail!("Bad song id")
        };

        let song_uuid = Uuid::parse_str(song_id)?;

        let mut song_path = project_dirs().cache_dir().to_path_buf();

        song_path.push(song_uuid.hyphenated().to_string());
        log::info!("Writing song cache {:?}", &song_path);
        std::fs::create_dir_all(&song_path);
        song_path.push("jacket.png");

        let song = self
            .all_songs
            .iter()
            .find(|x| x.id == SongId::StringId(song_id.clone()))
            .ok_or(anyhow!("song id not in song list"))?;

        let read = &song.difficulties.read().expect("Lock error");
        let diff = read
            .iter()
            .find(|x| x.id == *diff_id)
            .ok_or(anyhow!("diff id not in songs difficulties"))?;

        download_song(song_uuid, diff.difficulty, self.song_loaded.0.clone())
    }

    fn get_preview(
        &self,
        id: &SongId,
    ) -> poll_promise::Promise<
        anyhow::Result<(
            Box<dyn Source<Item = f32> + Send>,
            std::time::Duration,
            std::time::Duration,
        )>,
    > {
        let id = id.clone();
        poll_promise::Promise::spawn_async(async move {
            let SongId::StringId(song_id) = id else {
                bail!("Unsupported id type")
            };

            let song_uuid = Uuid::parse_str(&song_id)?;

            let mut song_path = project_dirs().cache_dir().to_path_buf();

            song_path.push(song_uuid.hyphenated().to_string());
            log::info!("Writing song cache {:?}", &song_path);
            std::fs::create_dir_all(&song_path)?;
            song_path.push("preview");

            let source: Box<dyn Source<Item = f32> + Send> = if song_path.exists() {
                Box::new(rodio::Decoder::new(std::fs::File::open(song_path)?)?.convert_samples())
            } else {
                let NauticaSong { data: nautica } = reqwest::get(format!(
                    "https://ksm.dev/app/songs/{}",
                    song_uuid.as_hyphenated()
                ))
                .await
                .expect("Failed to get song")
                .json()
                .await
                .expect("Failed to parse nautica song");

                let Some(preview_url) = nautica.preview_url else {
                    bail!("No preview url")
                };

                let mut bytes = reqwest::get(preview_url).await?.bytes().await?;

                std::fs::write(song_path, &bytes)?;

                Box::new(rodio::Decoder::new(std::io::Cursor::new(bytes))?.convert_samples())
            };
            Ok((
                source as Box<dyn Source<Item = f32> + Send>,
                Duration::ZERO,
                Duration::MAX,
            ))
        })
    }

    fn subscribe(&mut self) -> bus::BusReader<SongProviderEvent> {
        if self.next.is_none() {
            self.next = Some(Promise::spawn_async(next_songs(self.next_url.clone())));
        }

        self.bus.add_rx()
    }

    fn get_all(&self) -> (Vec<Arc<Song>>, Vec<SongId>) {
        (
            self.all_songs.clone(),
            self.all_songs.iter().map(|x| x.id.clone()).collect(),
        )
    }

    fn get_available_sorts(&self) -> Vec<super::SongSort> {
        vec![]
    }
}

fn download_song(id: Uuid, diff: u8, on_loaded: Sender<Datum>) -> anyhow::Result<LoadSongFn> {
    Ok(Box::new(move || {
        let mut song_path = project_dirs().cache_dir().to_path_buf();

        song_path.push(id.hyphenated().to_string());
        song_path.push("data.zip");

        if song_path.exists() {
            let file = File::open(song_path)?;
            let file = BufReader::new(file);
            return song_from_zip(file, diff);
        }

        let NauticaSong { data: nautica } =
            reqwest::blocking::get(format!("https://ksm.dev/app/songs/{}", id.as_hyphenated()))?
                .json()?;
        let mut data = reqwest::blocking::get(&nautica.cdn_download_url)?.bytes()?;
        std::fs::write(&song_path, data)?;

        let file = File::open(song_path)?;
        on_loaded.send(nautica);
        song_from_zip(BufReader::new(file), diff)
    }))
}

fn song_from_zip(
    data: impl std::io::Read + std::io::Seek,
    diff: u8,
) -> Result<(kson::Chart, Box<dyn rodio::Source<Item = f32> + Send>)> {
    let mut archive = zip::read::ZipArchive::new(data)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.is_dir() {
            continue;
        }
        let mut chart_string = String::new();
        let file_read = file.read_to_string(&mut chart_string);
        if file_read.is_err() {
            continue;
        }

        let file_folder = PathBuf::from(file.name());
        drop(file);

        if let Ok(chart) = kson::Chart::from_ksh(&chart_string) {
            if chart.meta.difficulty == diff {
                let bgm_name = chart.audio.bgm.filename.clone();
                let bgm_path = file_folder.with_file_name(bgm_name);
                let bgm_path = bgm_path.to_str().unwrap_or("").replace('\\', "/");

                log::info!("Loading {bgm_path}");

                let mut bgm_entry = archive.by_name(&bgm_path)?;
                let mut bgm_buf = Vec::new();
                bgm_entry.read_to_end(&mut bgm_buf)?;
                let bgm_cursor = std::io::Cursor::new(bgm_buf);

                return Ok((
                    chart,
                    Box::new(rodio::Decoder::new(bgm_cursor)?.convert_samples()),
                ));
            }
        }
    }
    Err(anyhow::anyhow!("Could not find difficulty in zip archive"))
}

use std::{
    collections::{HashMap, VecDeque},
    fmt::Debug,
    fs::File,
    io::{BufReader, BufWriter, Read},
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use rayon::prelude::*;
use rodio::Source;

use crate::{
    project_dirs,
    songselect::{Difficulty, Song},
    worker_service::WorkerService,
};

use super::{LoadSongFn, SongProvider, SongProviderEvent};
use anyhow::{bail, ensure, Result};
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
        song_path.push(id.hyphenated().to_string());
        let (id_0, id_1) = id.as_u64_pair();
        let song_id = id_0 ^ id_1;

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
            id: song_id,
            difficulties: charts
                .iter()
                .map(|x| x.as_diff(jacket_path.clone()))
                .collect(),
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

        let (id_0, id_1) = uid.as_u64_pair();
        let id = id_0 ^ id_1;

        Difficulty {
            jacket_path,
            level: *level as u8,
            difficulty: *difficulty as u8 - 1,
            id,
            effector: effector.clone(),
            top_badge: 0,
            scores: vec![],
            hash: Some(uid.to_string()),
        }
    }
}

pub struct NauticaSongProvider {
    next: Option<Promise<Result<NauticaSongs>>>,
    events: VecDeque<SongProviderEvent>,
    all_songs: Vec<Arc<Song>>,
    id_map: HashMap<u64, Uuid>,
    next_url: String,
    bus: bus::Bus<SongProviderEvent>,
}

impl Debug for NauticaSongProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NauticaSongProvider").finish()
    }
}

fn next_songs(path: &str) -> Promise<Result<NauticaSongs>> {
    log::info!("Getting more nautica songs: {}", path);
    let path = String::from_str(path).unwrap();
    Promise::spawn_thread("get nautica", move || {
        let nautica_songs = ureq::get(&path).call()?.into_json::<NauticaSongs>()?;
        nautica_songs
            .data
            .par_iter()
            .try_for_each(|x| -> Result<()> {
                let mut song_path = project_dirs().cache_dir().to_path_buf();
                song_path.push(x.id.hyphenated().to_string());
                std::fs::create_dir_all(&song_path)?;
                if x.jacket_url.ends_with("png") {
                    song_path.push("jacket.png");
                } else {
                    song_path.push("jacket.jpg");
                }

                if song_path.exists() {
                    return Ok(());
                }

                let jacket_response = ureq::get(&x.jacket_url).call().expect("No jacket");
                let jacket_path = match jacket_response.content_type() {
                    "image/jpeg" => song_path.with_extension("jpg"),
                    "image/png" => song_path.with_extension("png"),
                    "image/webp" => song_path.with_extension("webp"),
                    content_type => {
                        bail!("Can't load jackets of type: {content_type}");
                    }
                };

                let file = File::create(jacket_path)?;
                let mut file = std::io::BufWriter::new(file);
                std::io::copy(&mut jacket_response.into_reader(), &mut file)?;

                Ok(())
            })?;
        Ok(nautica_songs)
    })
}

impl NauticaSongProvider {
    pub fn new() -> Self {
        let first_songs = next_songs("https://ksm.dev/app/songs")
            .block_and_take()
            .unwrap();

        let mut events = VecDeque::new();
        let (new_songs, ids): (Vec<Arc<Song>>, Vec<(u64, Uuid)>) = first_songs
            .data
            .iter()
            .map(|d| {
                let song = d.as_song();
                let song_id = song.id;
                (Arc::new(song), (song_id, d.id))
            })
            .unzip();

        events.push_back(SongProviderEvent::SongsAdded(new_songs.clone()));

        Self {
            next: None,
            events,
            all_songs: new_songs,
            id_map: ids.iter().copied().collect(),
            next_url: first_songs.links.next.unwrap_or_default(),
            bus: bus::Bus::new(32),
        }
    }
}

impl WorkerService for NauticaSongProvider {
    fn update(&mut self) {
        if let Some(next) = self.next.take() {
            match next.try_take() {
                Ok(Ok(songs)) => {
                    let (new_songs, new_ids): (Vec<Arc<Song>>, Vec<(u64, Uuid)>) = songs
                        .data
                        .iter()
                        .map(|d| {
                            let data = d.as_song();
                            let song_id = data.id;
                            (Arc::new(data), (song_id, d.id))
                        })
                        .unzip();

                    self.id_map.extend(new_ids.iter().copied());
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

        for ele in self.events.drain(..) {
            self.bus.broadcast(ele);
        }
    }
}

impl SongProvider for NauticaSongProvider {
    fn set_search(&mut self, _query: &str) {
        todo!()
    }

    fn set_sort(&mut self, _sort: super::SongSort) {
        todo!()
    }

    fn set_filter(&mut self, _filter: super::SongFilter) {
        todo!()
    }

    fn set_current_index(&mut self, index: u64) {
        if self.next.is_some() {
            return;
        }

        if let Some((i, _)) = self.all_songs.iter().enumerate().find(|x| x.1.id == index) {
            if i > self.all_songs.len().saturating_sub(10) {
                self.next = Some(next_songs(&self.next_url));
            }
        }
    }

    fn load_song(
        &self,
        index: u64,
        diff_id: u64,
    ) -> Box<dyn FnOnce() -> (kson::Chart, Box<dyn rodio::Source<Item = f32> + Send>) + Send> {
        if let Some(song_uuid) = self.id_map.get(&index) {
            let mut song_path = project_dirs().cache_dir().to_path_buf();

            song_path.push(song_uuid.hyphenated().to_string());
            log::info!("Writing song cache {:?}", &song_path);
            std::fs::create_dir_all(&song_path);
            song_path.push("jacket.png");

            let song = self
                .all_songs
                .iter()
                .find(|x| x.id == index)
                .expect("song id not in song list");

            let diff = song
                .difficulties
                .iter()
                .find(|x| x.id == diff_id)
                .expect("diff id not in songs difficulties");

            download_song(*song_uuid, diff.difficulty)
        } else {
            todo!()
        }
    }

    fn get_preview(
        &self,
        id: u64,
    ) -> anyhow::Result<(
        Box<dyn Source<Item = f32> + Send>,
        std::time::Duration,
        std::time::Duration,
    )> {
        let song_uuid = self.id_map.get(&id);
        ensure!(song_uuid.is_some());
        let song_uuid = song_uuid.unwrap();
        let mut song_path = project_dirs().cache_dir().to_path_buf();

        song_path.push(song_uuid.hyphenated().to_string());
        log::info!("Writing song cache {:?}", &song_path);
        std::fs::create_dir_all(&song_path)?;
        song_path.push("preview");

        let source: Box<dyn Source<Item = f32> + Send> = if song_path.exists() {
            Box::new(rodio::Decoder::new(std::fs::File::open(song_path)?)?.convert_samples())
        } else {
            let NauticaSong { data: nautica } = ureq::get(&format!(
                "https://ksm.dev/app/songs/{}",
                song_uuid.as_hyphenated()
            ))
            .call()
            .expect("Failed to get song")
            .into_json()
            .expect("Failed to parse nautica song");

            ensure!(nautica.preview_url.is_some());
            let preview_url = nautica.preview_url.unwrap();

            let mut bytes = vec![];

            ureq::get(&preview_url)
                .call()?
                .into_reader()
                .read_to_end(&mut bytes)?;

            std::fs::write(song_path, &bytes)?;

            Box::new(rodio::Decoder::new(std::io::Cursor::new(bytes))?.convert_samples())
        };
        Ok((source, Duration::ZERO, Duration::MAX))
    }

    fn subscribe(&mut self) -> bus::BusReader<SongProviderEvent> {
        self.bus.add_rx()
    }

    fn get_all(&self) -> Vec<Arc<Song>> {
        self.all_songs.clone()
    }
}

fn download_song(id: Uuid, diff: u8) -> LoadSongFn {
    Box::new(move || {
        let mut song_path = project_dirs().cache_dir().to_path_buf();

        song_path.push(id.hyphenated().to_string());
        song_path.push("data.zip");

        if song_path.exists() {
            let file = File::open(song_path).unwrap();
            let file = BufReader::new(file);
            return song_from_zip(file, diff).expect("Failed to load song from zip");
        }

        let NauticaSong { data: nautica } =
            ureq::get(&format!("https://ksm.dev/app/songs/{}", id.as_hyphenated()))
                .call()
                .expect("Failed to get song")
                .into_json()
                .expect("Failed to parse nautica song");

        let mut data = ureq::get(&nautica.cdn_download_url)
            .call()
            .expect("Failed to download song zip")
            .into_reader();

        let file = File::create(&song_path).expect("Failed to create song zip for downloading");
        {
            let mut file_writer = BufWriter::new(&file);
            std::io::copy(&mut data, &mut file_writer);
        }
        drop(file);
        let file = File::open(song_path).expect("Ug");
        return song_from_zip(BufReader::new(file), diff).expect("Failed to load song from zip");
    })
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
                let bgm_name = chart.audio.bgm.clone().unwrap().filename.unwrap();
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

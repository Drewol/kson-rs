use std::{collections::VecDeque, fmt::Debug, path::PathBuf, str::FromStr, sync::Arc};

use crate::songselect::{Difficulty, Song};

use super::{SongProvider, SongProviderEvent};
use anyhow::Result;
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
            user_id,
            title,
            artist,
            jacket_filename,
            description,
            download_link,
            downloads,
            has_preview,
            hidden,
            mojibake,
            uploaded_at,
            created_at,
            updated_at,
            jacket_url,
            preview_url,
            cdn_download_url,
            user,
            charts,
            tags,
        } = self;

        let (id_0, id_1) = id.as_u64_pair();
        let id = id_0 ^ id_1;

        Song {
            title: title.clone(),
            artist: artist.clone(),
            bpm: "unk".to_string(),
            id,
            difficulties: charts.iter().map(Chart::as_diff).collect(),
        }
    }
}

impl Chart {
    fn as_diff(&self) -> Difficulty {
        let Chart {
            id,
            user_id,
            song_id,
            difficulty,
            level,
            effector,
            video_link,
            created_at,
            updated_at,
        } = self;

        let (id_0, id_1) = id.as_u64_pair();
        let id = id_0 ^ id_1;

        Difficulty {
            jacket_path: PathBuf::default(),
            level: *level as u8,
            difficulty: *difficulty as u8 - 1,
            id,
            effector: effector.clone(),
            best_badge: 0,
            scores: vec![],
        }
    }
}

pub struct NauticaSongProvider {
    next: Option<Promise<Result<NauticaSongs>>>,
    events: VecDeque<SongProviderEvent>,
    all_songs: Vec<Arc<Song>>,
    next_url: String,
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
        Ok(ureq::get(&path).call()?.into_json::<NauticaSongs>()?)
    })
}

impl NauticaSongProvider {
    pub fn new() -> Self {
        let first_songs = next_songs("https://ksm.dev/app/songs")
            .block_and_take()
            .unwrap();

        let mut events = VecDeque::new();
        let new_songs: Vec<Arc<Song>> = first_songs
            .data
            .iter()
            .map(|d| Arc::new(d.as_song()))
            .collect();
        events.push_back(SongProviderEvent::SongsAdded(new_songs.clone()));

        Self {
            next: None,
            events,
            all_songs: new_songs,
            next_url: first_songs.links.next.unwrap_or_default(),
        }
    }
}

impl SongProvider for NauticaSongProvider {
    fn poll(&mut self) -> Option<super::SongProviderEvent> {
        if let Some(next) = self.next.take() {
            match next.try_take() {
                Ok(Ok(songs)) => {
                    let new_songs: Vec<Arc<Song>> =
                        songs.data.iter().map(|d| Arc::new(d.as_song())).collect();
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

        self.events.pop_front()
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
        if self.next.is_some() {
            return;
        }

        if let Some((i, _)) = self.all_songs.iter().enumerate().find(|x| x.1.id == index) {
            if i > self.all_songs.len().saturating_sub(10) {
                self.next = Some(next_songs(&self.next_url));
            }
        }
    }

    fn load_song(&mut self, index: u64) -> poll_promise::Promise<anyhow::Result<kson::Chart>> {
        todo!()
    }
}

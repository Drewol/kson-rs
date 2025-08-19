#![allow(unused)]

use std::{
    collections::HashSet,
    default,
    fmt::{format, Debug, Display, Write},
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use anyhow::ensure;
use egui::util::hash;
use kson::Chart;
use log::LevelFilter;
use luals_gen::ToLuaLsType;
use mlua::UserData;
use poll_promise::Promise;
use rodio::Source;
use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};

use crate::{
    multiplayer,
    results::Score,
    songselect::{self, Song},
};
use specta::Type;
mod files;
mod nautica;

#[derive(Debug, Clone)]
pub enum SongProviderEvent {
    SongsAdded(Vec<Arc<Song>>),
    SongsRemoved(HashSet<SongId>),
    OrderChanged(Vec<SongId>),
    StatusUpdate(String),
}

#[derive(Debug, Clone)]
pub enum ScoreProviderEvent {
    NewScore(SongDiffId, Score), //(diff.id, score)
}

pub enum ScoreFilter {
    Local,
    Online,
    Mixed,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    schemars::JsonSchema,
    PartialEq,
    specta::Type,
)]
pub enum SortDir {
    #[default]
    Asc,
    Desc,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    schemars::JsonSchema,
    PartialEq,
    specta::Type,
)]
pub enum SongSortType {
    #[default]
    Title,
    Score,
    Date,
    Artist,
    Effector,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    schemars::JsonSchema,
    PartialEq,
    specta::Type,
)]
pub struct SongSort {
    pub sort_type: SongSortType,
    pub direction: SortDir,
}

impl From<SongSort> for (rusc_database::SortColumn, rusc_database::SortDir) {
    fn from(val: SongSort) -> Self {
        (
            match val.sort_type {
                SongSortType::Title => rusc_database::SortColumn::Title,
                SongSortType::Score => rusc_database::SortColumn::Score,
                SongSortType::Date => rusc_database::SortColumn::Date,
                SongSortType::Artist => rusc_database::SortColumn::Artist,
                SongSortType::Effector => rusc_database::SortColumn::Effector,
            },
            match val.direction {
                SortDir::Asc => rusc_database::SortDir::Asc,
                SortDir::Desc => rusc_database::SortDir::Desc,
            },
        )
    }
}

impl SongSort {
    pub fn new(sort_type: SongSortType, direction: SortDir) -> Self {
        Self {
            sort_type,
            direction,
        }
    }
}

impl Display for SongSort {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self.sort_type {
            SongSortType::Title => formatter.write_str("Title"),
            SongSortType::Score => formatter.write_str("Score"),
            SongSortType::Date => formatter.write_str("Date"),
            SongSortType::Artist => formatter.write_str("Artist"),
            SongSortType::Effector => formatter.write_str("Effector"),
        }?;

        formatter.write_str(" ")?;

        match self.direction {
            SortDir::Desc => formatter.write_str("v"),
            SortDir::Asc => formatter.write_str("^"),
        }
    }
}

#[derive(
    Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema, PartialEq, specta::Type,
)]
pub enum SongFilterType {
    #[default]
    None,
    Folder(String),
    Collection(String),
}

impl Display for SongFilterType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            SongFilterType::None => formatter.write_str("All"),
            SongFilterType::Folder(f) => formatter.write_fmt(format_args!("Folder: {f}")),
            SongFilterType::Collection(c) => formatter.write_fmt(format_args!("Collection: {c}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SongFilter {
    pub filter_type: SongFilterType,
    pub level: u8,
}

impl SongFilter {
    pub fn new(filter_type: SongFilterType, level: u8) -> Self {
        Self { filter_type, level }
    }
}

#[derive(
    Debug,
    Clone,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    ToLuaLsType,
    SerializeDisplay,
    DeserializeFromStr,
)]
pub enum SongId {
    Missing,
    IntId(i64),
    StringId(String),
}

impl FromStr for SongId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Missing" => Ok(SongId::Missing),
            s => {
                if s.starts_with("IntId") {
                    Ok(SongId::IntId(s[6..s.len() - 1].parse()?))
                } else {
                    ensure!(s.len() > 8);
                    Ok(SongId::StringId(s[8..s.len() - 1].to_string()))
                }
            }
        }
    }
}

impl Display for SongId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use std::fmt::*;
        match self {
            SongId::Missing => f.write_str("Missing"),
            SongId::IntId(v) => f.write_fmt(format_args!("IntId({v})")),
            SongId::StringId(v) => f.write_fmt(format_args!("StringId({v})")),
        }
    }
}

impl SongId {
    pub fn as_u64(&self) -> u64 {
        match self {
            SongId::Missing => u64::MAX,
            SongId::IntId(v) => *v as u64,
            SongId::StringId(s) => hash(s),
        }
    }
}

impl Default for SongId {
    fn default() -> Self {
        Self::Missing
    }
}

#[derive(
    Debug,
    Clone,
    DeserializeFromStr,
    SerializeDisplay,
    Default,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    ToLuaLsType,
)]
pub struct DiffId(pub SongId);

impl Display for DiffId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <SongId as Display>::fmt(&self.0, f)
    }
}

impl FromStr for DiffId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(SongId::from_str(s)?))
    }
}

#[derive(Debug, Clone, ToLuaLsType, Serialize, Deserialize)]
pub enum SongDiffId {
    Missing,
    DiffOnly(DiffId),
    SongDiff(SongId, DiffId),
}

impl Default for SongDiffId {
    fn default() -> Self {
        Self::Missing
    }
}

impl SongDiffId {
    pub fn get_diff(&self) -> Option<&DiffId> {
        match self {
            SongDiffId::Missing => None,
            SongDiffId::DiffOnly(d) => Some(d),
            SongDiffId::SongDiff(_, d) => Some(d),
        }
    }

    pub fn get_song(&self) -> Option<&SongId> {
        match self {
            SongDiffId::Missing => None,
            SongDiffId::DiffOnly(_) => None,
            SongDiffId::SongDiff(s, _) => Some(s),
        }
    }
}

pub type PreviewResult = anyhow::Result<(Box<dyn Source<Item = f32> + Send>, Duration, Duration)>;
pub type LoadSongFn = Box<
    dyn FnOnce() -> anyhow::Result<(
            Chart,
            Box<dyn rodio::Source<Item = f32> + Send>,
            Option<PathBuf>,
        )> + Send,
>;

pub trait SongProvider: Send {
    fn subscribe(&mut self) -> bus::BusReader<SongProviderEvent>;
    fn set_search(&mut self, query: &str);
    fn get_available_sorts(&self) -> Vec<SongSort>;
    fn get_available_filters(&self) -> Vec<SongFilterType>;
    fn set_sort(&mut self, sort: SongSort);
    fn set_filter(&mut self, filter: SongFilter);
    fn set_current_index(&mut self, index: u64);
    fn load_song(&self, id: &SongDiffId) -> anyhow::Result<LoadSongFn>;
    fn set_multiplayer_song(
        &self,
        id: &SongDiffId,
    ) -> anyhow::Result<multiplayer_protocol::messages::server::SetSong>;

    fn get_multiplayer_song(
        &self,
        hash: &str,
        path: &str,
        diff: u32,
        level: u32,
    ) -> anyhow::Result<Arc<Song>>;
    fn add_score(&self, id: SongDiffId, score: Score);
    /// Returns: `(music, skip, duration)`
    fn get_preview(&self, id: &SongId) -> Promise<PreviewResult>;
    fn get_all(&self) -> (Vec<Arc<Song>>, Vec<SongId>);
    fn refresh(&mut self) {}
    fn get_collections(&self, id: &SongId) -> Vec<songselect::favourite_dialog::Collection>;
    fn add_to_collection(&mut self, id: &SongId, collection: String) -> anyhow::Result<()>;
    fn remove_from_collection(&mut self, id: &SongId, collection: String) -> anyhow::Result<()>;
}

pub trait ScoreProvider {
    fn subscribe(&mut self) -> bus::BusReader<ScoreProviderEvent>;
    fn get_scores(&mut self, id: &SongDiffId) -> Vec<Score>;
    fn insert_score(&mut self, id: &SongDiffId, score: Score) -> anyhow::Result<()>;
    fn init_scores(&self, songs: &mut dyn Iterator<Item = &Arc<Song>>) -> anyhow::Result<()>;
}

pub use files::FileSongProvider;
pub use nautica::NauticaSongProvider;

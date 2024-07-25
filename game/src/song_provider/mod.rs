#![allow(unused)]

use std::{
    collections::HashSet,
    default,
    fmt::{format, Debug, Display, Write},
    sync::Arc,
    time::Duration,
};

use egui::util::hash;
use kson::Chart;
use log::LevelFilter;
use luals_gen::ToLuaLsType;
use rodio::Source;
use serde::{Deserialize, Serialize};
use tealr::{
    mlu::{mlua::UserData, TealData, UserData},
    ToTypename, TypeName,
};

use crate::{results::Score, songselect::Song};
mod files;
mod nautica;

#[derive(Debug, Clone)]
pub enum SongProviderEvent {
    SongsAdded(Vec<Arc<Song>>),
    SongsRemoved(HashSet<SongId>),
    OrderChanged(Vec<SongId>),
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum SortDir {
    #[default]
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum SongSortType {
    #[default]
    Title,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct SongSort {
    pub sort_type: SongSortType,
    pub direction: SortDir,
}

impl From<SongSort> for (rusc_database::SortColumn, rusc_database::SortDir) {
    fn from(val: SongSort) -> Self {
        (
            match val.sort_type {
                SongSortType::Title => rusc_database::SortColumn::Title,
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
        match (self.sort_type, self.direction) {
            (SongSortType::Title, SortDir::Asc) => formatter.write_str("Title ^"),
            (SongSortType::Title, SortDir::Desc) => formatter.write_str("Title v"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum SongFilterType {
    #[default]
    None,
    Folder(String),
    Collection(String),
}

impl Display for SongFilterType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            SongFilterType::None => formatter.write_str("None"),
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
    Debug, ToTypename, UserData, Clone, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord, ToLuaLsType,
)]
pub enum SongId {
    Missing,
    IntId(i64),
    StringId(String),
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

impl TealData for SongId {}

impl Default for SongId {
    fn default() -> Self {
        Self::Missing
    }
}

#[derive(
    Debug,
    ToTypename,
    UserData,
    Clone,
    Serialize,
    Default,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    ToLuaLsType,
)]
pub struct DiffId(pub SongId);

impl TealData for DiffId {}

#[derive(Debug, ToTypename, UserData, Clone, Serialize, ToLuaLsType)]
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

impl TealData for SongDiffId {}

pub type LoadSongFn =
    Box<dyn FnOnce() -> anyhow::Result<(Chart, Box<dyn rodio::Source<Item = f32> + Send>)> + Send>;

pub trait SongProvider {
    fn subscribe(&mut self) -> bus::BusReader<SongProviderEvent>;
    fn set_search(&mut self, query: &str);
    fn get_available_sorts(&self) -> Vec<SongSort>;
    fn get_available_filters(&self) -> Vec<SongFilterType>;
    fn set_sort(&mut self, sort: SongSort);
    fn set_filter(&mut self, filter: SongFilter);
    fn set_current_index(&mut self, index: u64);
    fn load_song(&self, id: &SongDiffId) -> anyhow::Result<LoadSongFn>;
    fn add_score(&self, id: SongDiffId, score: Score);
    /// Returns: `(music, skip, duration)`
    fn get_preview(
        &self,
        id: &SongId,
    ) -> anyhow::Result<(Box<dyn Source<Item = f32> + Send>, Duration, Duration)>;
    fn get_all(&self) -> Vec<Arc<Song>>;
}

pub trait ScoreProvider {
    fn subscribe(&mut self) -> bus::BusReader<ScoreProviderEvent>;
    fn get_scores(&mut self, id: &SongDiffId) -> Vec<Score>;
    fn insert_score(&mut self, id: &SongDiffId, score: Score) -> anyhow::Result<()>;
    fn init_scores(&self, songs: &mut dyn Iterator<Item = &Arc<Song>>) -> anyhow::Result<()>;
}

pub use files::FileSongProvider;
pub use nautica::NauticaSongProvider;

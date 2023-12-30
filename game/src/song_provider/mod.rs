#![allow(unused)]

use std::{collections::HashSet, fmt::Debug, sync::Arc, time::Duration};

use egui::util::hash;
use kson::Chart;
use rodio::Source;
use serde::Serialize;
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
    OrderChanged(Vec<u64>),
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

pub enum SortDir {
    Asc,
    Desc,
}

pub enum SongSort {
    Title(SortDir),
}

pub enum SongFilter {
    Level(u8),
    Folder(String),
    Collection(String),
}

#[derive(Debug, ToTypename, UserData, Clone, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord)]
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
    Debug, ToTypename, UserData, Clone, Serialize, Default, Hash, PartialEq, Eq, PartialOrd, Ord,
)]
pub struct DiffId(pub SongId);

impl TealData for DiffId {}

#[derive(Debug, ToTypename, UserData, Clone, Serialize)]
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

type LoadSongFn = Box<dyn FnOnce() -> (Chart, Box<dyn rodio::Source<Item = f32> + Send>) + Send>;

pub trait SongProvider {
    fn subscribe(&mut self) -> bus::BusReader<SongProviderEvent>;
    fn set_search(&mut self, query: &str);
    fn set_sort(&mut self, sort: SongSort);
    fn set_filter(&mut self, filter: SongFilter);
    fn set_current_index(&mut self, index: u64);
    fn load_song(&self, id: &SongDiffId) -> LoadSongFn;
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
    fn init_scores(&self, songs: &Vec<Arc<Song>>) -> anyhow::Result<()>;
}

pub use files::FileSongProvider;
pub use nautica::NauticaSongProvider;

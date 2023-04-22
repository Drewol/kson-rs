use std::{collections::HashSet, fmt::Debug, sync::Arc};


use kson::Chart;


use crate::songselect::Song;
mod files;
mod nautica;

#[derive(Debug)]
pub enum SongProviderEvent {
    SongsAdded(Vec<Arc<Song>>),
    SongsRemoved(HashSet<u64>),
    OrderChanged(Vec<u64>),
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

pub trait SongProvider: Debug {
    fn poll(&mut self) -> Option<SongProviderEvent>;
    fn set_search(&mut self, query: &str);
    fn set_sort(&mut self, sort: SongSort);
    fn set_filter(&mut self, filter: SongFilter);
    fn set_current_index(&mut self, index: u64);
    fn load_song(
        &self,
        song_index: u64,
        diff_index: u64,
    ) -> Box<dyn FnOnce() -> (Chart, Box<dyn rodio::Source<Item = i16>>) + Send>;
}

pub use files::FileSongProvider;
pub use nautica::NauticaSongProvider;

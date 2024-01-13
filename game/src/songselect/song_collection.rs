use itertools::Itertools;
use serde::{ser::SerializeSeq, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    ops::Index,
    slice::Iter,
    sync::Arc,
};

use crate::song_provider::SongId;

use super::Song;

#[derive(Debug, Default)]
pub struct SongCollection {
    songs: HashMap<SongId, Arc<Song>>,
    order: Vec<SongId>,
}

pub struct SongCollectionIter<'a> {
    iter: Iter<'a, SongId>,
    songs: &'a HashMap<SongId, Arc<Song>>,
}

impl SongCollection {
    pub fn remove(&mut self, ids: HashSet<SongId>) {
        self.order.retain(|x| !ids.contains(x));
        self.songs.retain(|x, _| !ids.contains(x));
    }
    pub fn set_order(&mut self, order: Vec<SongId>) {
        self.order = order;
    }
    pub fn append(&mut self, mut songs: Vec<Arc<Song>>) {
        for song in songs.drain(..) {
            self.order.push(song.id.clone());
            self.songs.insert(song.id.clone(), song);
        }
    }
    pub fn find_index(&self, id: SongId) -> Option<usize> {
        self.order.iter().find_position(|x| **x == id).map(|x| x.0)
    }

    pub fn add(&mut self, songs: Vec<Arc<Song>>, order: Vec<SongId>) {
        self.order = order;
        for song in songs.iter() {
            self.songs.insert(song.id.clone(), song.clone());
        }
    }
    pub fn get(&self, index: usize) -> Option<&Arc<Song>> {
        self.order.get(index).and_then(|id| self.songs.get(id))
    }
    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn iter(&self) -> SongCollectionIter<'_> {
        SongCollectionIter {
            iter: self.order.iter(),
            songs: &self.songs,
        }
    }
}

impl<'a> Iterator for SongCollectionIter<'a> {
    type Item = &'a Arc<Song>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().and_then(|id| self.songs.get(id))
    }
}

impl Serialize for SongCollection {
    fn serialize<S>(&self, serializer: S) -> std::prelude::v1::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.len()))?;
        for song in self.iter() {
            seq.serialize_element(song)?;
        }

        seq.end()
    }
}

impl Index<usize> for SongCollection {
    type Output = Arc<Song>;

    fn index(&self, index: usize) -> &Self::Output {
        self.songs.get(&self.order[index]).unwrap()
    }
}

#![allow(unused)]
use std::{
    sync::{mpsc::Sender, Arc, Weak},
    time::Duration,
};

use rodio::{Sample, Source};

#[derive(Clone)]
pub struct Marker(Arc<()>);

impl Marker {
    pub fn new() -> Self {
        Self(Arc::new(()))
    }

    pub fn get_handle(&self) -> MarkerHandle {
        MarkerHandle(Arc::downgrade(&self.0))
    }
}

impl Default for Marker {
    fn default() -> Self {
        Self::new()
    }
}

pub struct MarkerHandle(Weak<()>);

impl MarkerHandle {
    fn is_dead(&self) -> bool {
        self.0.strong_count() == 0
    }

    fn is_alive(&self) -> bool {
        self.0.strong_count() > 0
    }
}

pub fn owned_source<I>(source: I, owner: &Marker) -> OwnedSource<I>
where
    I: Source,
    I::Item: Sample,
{
    let update_frequency = (20 * source.sample_rate()) / 1000 * source.channels() as u32;
    OwnedSource {
        owner: owner.get_handle(),
        input: source,
        closed: false,
        samples_until_check: update_frequency,
        update_frequency,
    }
}

pub struct OwnedSource<I> {
    owner: MarkerHandle,
    input: I,
    closed: bool,
    samples_until_check: u32,
    update_frequency: u32,
}

impl<I> OwnedSource<I>
where
    I: Source,
    I::Item: Sample,
{
    /// Returns a reference to the inner source.
    #[inline]
    pub const fn inner(&self) -> &I {
        &self.input
    }

    /// Returns a mutable reference to the inner source.
    #[inline]
    pub fn inner_mut(&mut self) -> &mut I {
        &mut self.input
    }

    /// Returns the inner source.
    #[inline]
    pub fn into_inner(self) -> I {
        self.input
    }
}

impl<I> Iterator for OwnedSource<I>
where
    I: Source,
    I::Item: Sample,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.samples_until_check -= 1;
        if self.samples_until_check == 0 {
            self.closed = self.owner.is_dead();
            self.samples_until_check = self.update_frequency;
        }

        if self.closed {
            None
        } else {
            self.input.next()
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.input.size_hint()
    }
}

impl<I> Source for OwnedSource<I>
where
    I: Source,
    I::Item: Sample,
{
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        self.input.current_frame_len()
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.input.channels()
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.input.sample_rate()
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }
}

use std::sync::{Arc, RwLock};

use rodio::{Sample, Source};

pub struct TakeableSource<I: Source<Item = D> + Send, D: Sample> {
    source: Arc<RwLock<Option<I>>>,
    channels: u16,
    sample_rate: u32,
}

impl<I: Source<Item = D> + Send, D: Sample> TakeableSource<I, D> {
    pub fn new(source: I) -> (Self, Arc<RwLock<Option<I>>>) {
        let channels = source.channels();
        let sample_rate = source.sample_rate();
        let source = Arc::new(RwLock::new(Some(source)));
        (
            Self {
                source: source.clone(),
                channels,
                sample_rate,
            },
            source,
        )
    }
}

impl<I, D> Iterator for TakeableSource<I, D>
where
    I: Source<Item = D> + Send,
    D: Sample,
{
    type Item = D;

    fn next(&mut self) -> Option<Self::Item> {
        self.source
            .write()
            .ok()
            .as_mut()
            .and_then(|x| x.as_mut().and_then(|x| x.next()))
    }
}

impl<I, D> Source for TakeableSource<I, D>
where
    I: Source<Item = D> + Send,
    D: Sample,
{
    fn current_frame_len(&self) -> Option<usize> {
        if let Ok(s) = self.source.read() {
            s.as_ref().and_then(|s| s.current_frame_len())
        } else {
            None
        }
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        if let Ok(s) = self.source.read() {
            s.as_ref().and_then(|s| s.total_duration())
        } else {
            None
        }
    }
}

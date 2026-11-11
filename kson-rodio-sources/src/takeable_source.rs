use rodio::{ChannelCount, SampleRate};
use rodio::{Sample, Source};
use std::sync::{Arc, RwLock};

pub struct TakeableSource<I: Source + Send> {
    source: Arc<RwLock<Option<I>>>,
    channels: ChannelCount,
    sample_rate: SampleRate,
}

impl<I: Source + Send> TakeableSource<I> {
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

impl<I> Iterator for TakeableSource<I>
where
    I: Source + Send,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.source
            .write()
            .ok()
            .as_mut()
            .and_then(|x| x.as_mut().and_then(|x| x.next()))
    }
}

impl<I> Source for TakeableSource<I>
where
    I: Source + Send,
{
    fn current_span_len(&self) -> Option<usize> {
        if let Ok(s) = self.source.read() {
            s.as_ref().and_then(|s| s.current_span_len())
        } else {
            None
        }
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
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

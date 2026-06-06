use rodio::{ChannelCount, SampleRate};
use rodio::{Sample, Source};
use std::time::Duration;

use rodio::source::Delay;

pub trait MixSource: Source {
    fn set_mix(&mut self, mix: f32);
}

impl Source for Box<dyn MixSource> {
    fn current_span_len(&self) -> Option<usize> {
        (**self).current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        (**self).channels()
    }

    fn sample_rate(&self) -> SampleRate {
        (**self).sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        (**self).total_duration()
    }
}

impl Source for Box<dyn MixSource + Send> {
    fn current_span_len(&self) -> Option<usize> {
        (**self).current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        (**self).channels()
    }

    fn sample_rate(&self) -> SampleRate {
        (**self).sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        (**self).total_duration()
    }
}

impl Source for Box<dyn MixSource + Send + Sync> {
    fn current_span_len(&self) -> Option<usize> {
        (**self).current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        (**self).channels()
    }

    fn sample_rate(&self) -> SampleRate {
        (**self).sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        (**self).total_duration()
    }
}

impl MixSource for Box<dyn MixSource> {
    fn set_mix(&mut self, mix: f32) {
        (**self).set_mix(mix);
    }
}

impl MixSource for Box<dyn MixSource + Send> {
    fn set_mix(&mut self, mix: f32) {
        (**self).set_mix(mix);
    }
}

impl MixSource for Box<dyn MixSource + Send + Sync> {
    fn set_mix(&mut self, mix: f32) {
        (**self).set_mix(mix);
    }
}

pub struct NoMix<I: Source>(pub I);

impl<I> Iterator for NoMix<I>
where
    I: Source,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

impl<I> Source for NoMix<I>
where
    I: Source,
{
    fn current_span_len(&self) -> Option<usize> {
        self.0.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.0.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.0.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.0.total_duration()
    }
}

impl<I> MixSource for NoMix<I>
where
    I: Source,
{
    fn set_mix(&mut self, _mix: f32) {}
}
impl<I> MixSource for Delay<I>
where
    I: MixSource,
{
    fn set_mix(&mut self, mix: f32) {
        self.inner_mut().set_mix(mix);
    }
}

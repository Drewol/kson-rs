#![allow(unused)]
use rodio::{ChannelCount, SampleRate};
use rodio::{Sample, Source};
use std::time::Duration;

use super::mix_source::MixSource;
use rodio::source::UniformSourceIterator;

pub fn effected_part<E: MixSource>(
    effected: E,
    skip: Duration,
    take: Duration,
    base_mix: f32,
) -> EffectedPart<E> {
    let target_sample_rate = effected.sample_rate().get();
    let target_channels = effected.channels().get();

    EffectedPart {
        effected,
        skip: ((skip.as_nanos() * target_sample_rate as u128 * target_channels as u128)
            / 1_000_000_000) as u64,
        take: ((take.as_nanos() * target_sample_rate as u128 * target_channels as u128)
            / 1_000_000_000) as u64,
        base_mix,
    }
}

pub struct EffectedPart<E>
where
    E: MixSource,
{
    effected: E,
    skip: u64,
    take: u64,
    base_mix: f32,
}

impl<E> Iterator for EffectedPart<E>
where
    E: MixSource,
{
    type Item = E::Item;

    fn next(&mut self) -> Option<Self::Item> {
        if self.take == 0 {
            self.effected.set_mix(0.0);
        } else if self.skip > 0 {
            self.effected.set_mix(0.0);
            self.skip -= 1;
        } else {
            self.take -= 1;
            self.effected.set_mix(self.base_mix);
        }

        self.effected.next()
    }
}

impl<E> Source for EffectedPart<E>
where
    E: MixSource,
{
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        self.effected.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.effected.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.effected.total_duration()
    }
}

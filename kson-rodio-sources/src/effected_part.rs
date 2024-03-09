#![allow(unused)]
use std::time::Duration;

use super::mix_source::MixSource;
use rodio::{source::UniformSourceIterator, Sample, Source};

pub fn effected_part<E: MixSource<Item = D>, D: Sample>(
    effected: E,
    skip: Duration,
    take: Duration,
    base_mix: f32,
) -> EffectedPart<E, D> {
    let target_sample_rate = effected.sample_rate();
    let target_channels = effected.channels();

    EffectedPart {
        effected,
        skip: ((skip.as_nanos() * target_sample_rate as u128 * target_channels as u128)
            / 1_000_000_000) as u64,
        take: ((take.as_nanos() * target_sample_rate as u128 * target_channels as u128)
            / 1_000_000_000) as u64,
        base_mix,
    }
}

pub struct EffectedPart<E, D>
where
    E: MixSource<Item = D>,
    D: Sample,
{
    effected: E,
    skip: u64,
    take: u64,
    base_mix: f32,
}

impl<E, D> Iterator for EffectedPart<E, D>
where
    E: MixSource<Item = D>,
    D: Sample,
{
    type Item = D;

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

impl<E, D> Source for EffectedPart<E, D>
where
    E: MixSource<Item = D>,
    D: Sample,
{
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.effected.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.effected.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.effected.total_duration()
    }
}

#![allow(unused)]
use std::time::Duration;

use rodio::{source::UniformSourceIterator, Sample, Source};

pub fn effected_part<I: Source<Item = D>, E: Source<Item = D>, D: Sample>(
    original: I,
    effected: E,
    skip: Duration,
    take: Duration,
) -> EffectedPart<I, E, D> {
    let target_sample_rate = original.sample_rate();
    let target_channels = original.channels();

    EffectedPart {
        original,
        effected: UniformSourceIterator::new(effected, target_channels, target_sample_rate),
        skip: ((skip.as_nanos() * target_sample_rate as u128 * target_channels as u128)
            / 1_000_000_000) as u64,
        take: ((take.as_nanos() * target_sample_rate as u128 * target_channels as u128)
            / 1_000_000_000) as u64,
    }
}

pub struct EffectedPart<I, E, D>
where
    I: Source<Item = D>,
    E: Source<Item = D>,
    D: Sample,
{
    original: I,
    effected: UniformSourceIterator<E, D>,
    skip: u64,
    take: u64,
}

impl<I, E, D> Iterator for EffectedPart<I, E, D>
where
    I: Source<Item = D>,
    E: Source<Item = D>,
    D: Sample,
{
    type Item = D;

    fn next(&mut self) -> Option<Self::Item> {
        let (original, effected) = (
            self.original.next(),
            if self.take == 0 {
                None
            } else {
                self.effected.next()
            },
        );
        if self.take == 0 {
            original
        } else if self.skip > 0 {
            self.skip -= 1;
            original
        } else {
            self.take -= 1;
            original.and(effected)
        }
    }
}

impl<I, E, D> Source for EffectedPart<I, E, D>
where
    I: Source<Item = D>,
    E: Source<Item = D>,
    D: Sample,
{
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.original.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.original.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.original.total_duration()
    }
}

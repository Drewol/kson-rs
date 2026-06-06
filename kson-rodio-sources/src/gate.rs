use rodio::{ChannelCount, SampleRate};
use rodio::{Sample, Source};
use std::ops::Mul;
use std::time::Duration;

use super::mix_source::MixSource;

pub struct Gate<I: Source> {
    input: I,
    cursor: u64,
    length: u64,
    gated_after: u64,
    countdown: u128,
    mix: f32,
    amount: f32,
}

pub fn gate<I: Source>(
    source: I,
    start: Duration,
    duration: Duration,
    gate: f64,
    amount: f32,
) -> Gate<I> {
    let channels = source.channels().get() as f64;
    let sample_rate = source.sample_rate().get() as f64;

    Gate {
        input: source,
        cursor: 0,
        length: (duration.as_secs_f64() * channels * sample_rate) as _,
        gated_after: (duration.as_secs_f64() * channels * sample_rate * gate) as _,
        countdown: (start.as_secs_f64() * channels * sample_rate) as _,
        mix: 1.0,
        amount,
    }
}

impl<I> Iterator for Gate<I>
where
    I: Source,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let original = self.input.next();
        if self.length == 0 {
            return original;
        }

        if self.countdown > 0 || self.mix < f32::EPSILON {
            self.countdown = self.countdown.saturating_sub(1);
            return original;
        }

        self.cursor = (self.cursor + 1) % self.length;
        let mix = if self.cursor > self.gated_after {
            self.amount * self.mix + (1.0 - self.mix)
        } else {
            1.0
        };

        original.map(|x| x.mul(mix))
    }
}

impl<I> Source for Gate<I>
where
    I: Source,
{
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.input.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }
}

impl<I> MixSource for Gate<I>
where
    I: Source,
{
    fn set_mix(&mut self, mix: f32) {
        self.mix = mix;
    }
}

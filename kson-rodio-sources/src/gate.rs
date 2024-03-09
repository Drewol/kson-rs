use std::time::Duration;

use rodio::{Sample, Source};

use super::mix_source::MixSource;

pub struct Gate<I: Source<Item = D>, D: Sample> {
    input: I,
    cursor: u64,
    length: u64,
    gated_after: u64,
    countdown: u128,
    mix: f32,
    amount: f32,
}

pub fn gate<I: Source<Item = D>, D: Sample>(
    source: I,
    start: Duration,
    duration: Duration,
    gate: f64,
    amount: f32,
) -> Gate<I, D> {
    let channels = source.channels() as f64;
    let sample_rate = source.sample_rate() as f64;

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

impl<I, D> Iterator for Gate<I, D>
where
    I: Source<Item = D>,
    D: Sample,
{
    type Item = D;

    fn next(&mut self) -> Option<Self::Item> {
        let original = self.input.next();
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

        original.map(|x| x.amplify(mix))
    }
}

impl<I, D> Source for Gate<I, D>
where
    I: Source<Item = D>,
    D: Sample,
{
    fn current_frame_len(&self) -> Option<usize> {
        self.input.current_frame_len()
    }

    fn channels(&self) -> u16 {
        self.input.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }
}

impl<I, D> MixSource for Gate<I, D>
where
    I: Source<Item = D>,
    D: Sample,
{
    fn set_mix(&mut self, mix: f32) {
        self.mix = mix;
    }
}

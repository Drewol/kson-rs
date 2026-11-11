use rodio::{ChannelCount, SampleRate};
use rodio::{Sample, Source};
use std::time::Duration;

use super::mix_source::MixSource;

pub fn tape_stop<I: Source>(input: I, start: Duration, duration: Duration) -> TapeStop<I> {
    let duration = duration.as_secs_f64();
    let sample_rate = input.sample_rate();
    let step = 1.0f64 / sample_rate.get() as f64;
    let channels = input.channels();
    let sample_count = (duration * input.sample_rate().get() as f64) as u64;
    let held_samples = Vec::with_capacity((sample_count * channels.get() as u64) as _);

    TapeStop {
        input,
        sample_advance: 1.0,
        held_samples,
        channel: 0,
        channels,
        duration,
        countdown: duration,
        step,
        mix: 1.0,
        sample_count,
        cursor: 0,
        start_countdown: (start.as_secs_f64() * sample_rate.get() as f64 * channels.get() as f64)
            as u128,
    }
}

pub struct TapeStop<I: Source> {
    input: I,
    sample_advance: f64,
    held_samples: Vec<Option<I::Item>>,
    channel: u16,
    channels: ChannelCount,
    duration: f64,
    countdown: f64,
    step: f64,
    mix: f32,
    sample_count: u64,
    cursor: usize,
    start_countdown: u128,
}

impl<I> Iterator for TapeStop<I>
where
    I: Source,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let original = self.input.next();
        if self.start_countdown > 0 || self.mix < f32::EPSILON {
            self.start_countdown = self.start_countdown.saturating_sub(1);
            return original;
        }

        if self.held_samples.len() < self.channels.get() as usize * self.sample_count as usize {
            self.held_samples.push(original)
        };

        let c = self.channel as usize;
        self.channel += 1;
        if self.channel >= self.channels.get() {
            self.channel = 0;
            if self.countdown > 0.0 {
                self.countdown -= self.step;
                self.sample_advance -= self.countdown / self.duration;

                while self.sample_advance <= 0.0 {
                    self.sample_advance += 1.0;
                    self.cursor += 1;
                }
            }
        }

        match (
            self.held_samples
                .get(c + self.cursor * self.channels.get() as usize)
                .copied()
                .flatten(),
            original,
        ) {
            (None, None) => None,
            (None, Some(v)) => Some(v),
            (Some(_), None) => None,
            (Some(effected), Some(original)) => Some(crate::lerp(
                original,
                effected,
                (1000.0 * self.mix) as u32,
                1000,
            )),
        }
    }
}

impl<I> Source for TapeStop<I>
where
    I: Source,
{
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}
impl<I> MixSource for TapeStop<I>
where
    I: Source,
{
    fn set_mix(&mut self, mix: f32) {
        self.mix = mix;
    }
}

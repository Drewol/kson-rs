use std::time::Duration;

use itertools::Itertools;
use rodio::{Sample, Source};

use super::mix_source::MixSource;

pub fn re_trigger<I: Source<Item = D>, D: Sample>(
    source: I,
    start: Duration,
    repeat_period: Duration,
    update_period: Duration,
    feedback: f32,
) -> ReTrigger<I, D> {
    let channels = source.channels();
    let sample_rate = source.sample_rate();
    ReTrigger {
        input: source,
        sample_buffer: (0..channels).map(|_| vec![]).collect_vec(),
        buffer_cursor: 0,
        repeat_period: (sample_rate as f64 * repeat_period.as_secs_f64()) as _,
        update_period: (sample_rate as f64 * update_period.as_secs_f64()) as _,
        update_counter: 0,
        channels,
        current_channel: 0,
        sample_rate,
        mix: 1.0,
        volume: 1.0,
        feedback,
        repeats: 0,
        countdown: (start.as_secs_f64() * sample_rate as f64 * channels as f64) as u128,
    }
}

pub struct ReTrigger<I: Source<Item = D>, D: Sample> {
    input: I,
    sample_buffer: Vec<Vec<Option<D>>>,
    buffer_cursor: usize,
    repeat_period: usize,
    update_period: usize,
    update_counter: usize,
    channels: u16,
    current_channel: u16,
    sample_rate: u32,
    mix: f32,
    volume: f32,
    feedback: f32,
    repeats: u32,
    countdown: u128,
}

impl<I, D> Iterator for ReTrigger<I, D>
where
    I: Source<Item = D>,
    D: Sample,
{
    type Item = D;

    fn next(&mut self) -> Option<Self::Item> {
        let original = self.input.next();
        if self.countdown > 0 {
            self.countdown -= 1;
            return original;
        }

        if self.update_counter == 0 && self.update_period > 0 {
            self.update_counter = self.update_period;
            for ele in self.sample_buffer.iter_mut() {
                ele.clear()
            }
            self.volume = 1.0;
            self.repeats = 0;
        }

        let buffer = &mut self.sample_buffer[self.current_channel as usize];

        if buffer.len() < self.repeat_period {
            buffer.push(original)
        }

        let mut effected = buffer.get(self.buffer_cursor).copied().flatten();
        if let Some(effected) = effected.as_mut() {
            effected.amplify(self.volume);
        }

        if self.current_channel == 0 {
            self.buffer_cursor = (self.buffer_cursor + 1) % self.repeat_period;
            if self.update_period > 0 {
                self.update_counter -= 1;
            }
        }

        self.current_channel = (self.current_channel + 1) % self.channels;

        if self.buffer_cursor == 0 {
            self.repeats += 1;
            self.volume = self.feedback.powi(self.repeats as _);
        }

        match (original, effected) {
            (None, None) => None,
            (None, Some(_)) => None,
            (Some(v), None) => Some(v),
            (Some(original), Some(effected)) => Some(Sample::lerp(
                original,
                effected,
                (self.mix * 1000.0) as u32,
                1000,
            )),
        }
    }
}

impl<I, D> Source for ReTrigger<I, D>
where
    I: Source<Item = D>,
    D: Sample,
{
    fn current_frame_len(&self) -> Option<usize> {
        self.input.current_frame_len()
    }

    fn channels(&self) -> u16 {
        self.channels as u16
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }
}

impl<I, D> MixSource for ReTrigger<I, D>
where
    I: Source<Item = D>,
    D: Sample,
{
    fn set_mix(&mut self, mix: f32) {
        self.mix = mix;
    }
}

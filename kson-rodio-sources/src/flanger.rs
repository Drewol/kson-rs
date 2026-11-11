use rodio::{ChannelCount, SampleRate};
use rodio::{Sample, Source};
use std::time::Duration;

use super::{mix_source::MixSource, triangle::TriangleWave};

pub fn flanger<I: Source + Send>(
    source: I,
    depth: Duration,
    delay: Duration,
    frequency: f32,
    separation: f32,
) -> Flanger<I> {
    let target_channels = source.channels().get();
    let target_sample_rate = source.sample_rate().get();
    let sample_depth = ((target_sample_rate as u128 * depth.as_nanos()) / 1_000_000_000) as usize;
    let sample_delay = ((target_sample_rate as u128 * delay.as_nanos()) / 1_000_000_000) as usize;

    Flanger {
        channels: source.channels(),
        sample_rate: source.sample_rate(),
        input: source,
        sample_buffer: vec![vec![I::Item::default(); target_channels as usize]; sample_depth],
        depth: sample_depth,
        delay: sample_delay * target_channels as usize,
        current_channel: 0,
        buffer_cursor: 0,
        mix: 1.0,
        cursors: (0..target_channels)
            .map(|i| {
                TriangleWave::new(
                    frequency,
                    0.5,
                    SampleRate::new(target_sample_rate).expect("Invalid sample rate"),
                    (i % 2) as f32 * separation,
                )
            })
            .collect(),
    }
}

pub struct Flanger<I>
where
    I: Source + Send,
{
    input: I,
    sample_buffer: Vec<Vec<I::Item>>,
    buffer_cursor: usize,
    depth: usize,
    delay: usize,
    channels: ChannelCount,
    current_channel: usize,
    sample_rate: SampleRate,
    cursors: Vec<TriangleWave>,
    mix: f32,
}

impl<I> Iterator for Flanger<I>
where
    I: Source + Send,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let ret = self.input.next();
        self.delay = self.delay.saturating_sub(1);

        if self.delay > 0 || self.mix < f32::EPSILON {
            return ret;
        }

        if let Some(sample) = ret {
            self.sample_buffer[self.buffer_cursor][self.current_channel] = sample;

            let delayed_buffer_cursor = (self.buffer_cursor as i64
                - ((self.cursors[self.current_channel].next()? + 0.5) * (self.depth - 1) as f32)
                    as i64)
                .rem_euclid(self.sample_buffer.len() as i64);

            let delayed_sample =
                self.sample_buffer[delayed_buffer_cursor as usize][self.current_channel];

            self.current_channel += 1;

            if self.current_channel >= self.channels.get() as usize {
                //Advance cursor
                self.buffer_cursor += 1;
                self.current_channel = 0;
            }

            if self.buffer_cursor >= self.sample_buffer.len() {
                self.buffer_cursor = 0;
            }

            Some(crate::lerp(
                sample,
                delayed_sample,
                (1000.0 * self.mix) as u32,
                2000,
            ))
        } else {
            None
        }
    }
}

impl<I> Source for Flanger<I>
where
    I: Source + Send,
{
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }
}

//new source, (start,end,buffered original?, effect source)
impl<I> MixSource for Flanger<I>
where
    I: Source + Send,
{
    fn set_mix(&mut self, mix: f32) {
        self.mix = mix;
    }
}

use std::time::Duration;

use itertools::Itertools;
use rodio::{cpal::FromSample, source::UniformSourceIterator, Sample, Source};

use super::triangle::TriangleWave;

pub fn flanger<I: Source<Item = D>, D: Sample>(
    source: I,
    depth: Duration,
    delay: Duration,
    frequency: f32,
    separation: f32,
) -> Flanger<I, D> {
    let target_channels = source.channels();
    let target_sample_rate = source.sample_rate();
    let sample_depth = ((target_sample_rate as u128 * depth.as_nanos()) / 1_000_000_000) as usize;
    let sample_delay = ((target_sample_rate as u128 * delay.as_nanos()) / 1_000_000_000) as usize;

    Flanger {
        input: UniformSourceIterator::new(source, target_channels, target_sample_rate),
        sample_buffer: vec![vec![D::zero_value(); target_channels as usize]; sample_depth],
        depth: sample_depth,
        delay: sample_delay * target_channels as usize,
        channels: target_channels as usize,
        sample_rate: target_sample_rate,
        current_channel: 0,
        buffer_cursor: 0,
        cursors: (0..target_channels)
            .map(|i| {
                TriangleWave::new(
                    frequency,
                    0.5,
                    target_sample_rate,
                    (i % 2) as f32 * separation,
                )
            })
            .collect_vec(),
    }
}

pub struct Flanger<I, D>
where
    I: Source,
    I::Item: Sample,
    D: Sample,
{
    input: UniformSourceIterator<I, D>,
    sample_buffer: Vec<Vec<D>>,
    buffer_cursor: usize,
    depth: usize,
    delay: usize,
    channels: usize,
    current_channel: usize,
    sample_rate: u32,
    cursors: Vec<TriangleWave>,
}

impl<I, D> Iterator for Flanger<I, D>
where
    I: Source,
    I::Item: Sample,
    D: FromSample<I::Item> + Sample,
{
    type Item = D;

    fn next(&mut self) -> Option<Self::Item> {
        let ret = self.input.next();
        self.delay = self.delay.saturating_sub(1);

        if self.delay > 0 {
            return ret;
        }

        if let Some(sample) = ret {
            self.sample_buffer[self.buffer_cursor][self.current_channel] = sample;

            let delayed_buffer_cursor = (self.buffer_cursor as i64
                - ((self.cursors[self.current_channel].next().unwrap() + 0.5)
                    * (self.depth - 1) as f32) as i64)
                .rem_euclid(self.sample_buffer.len() as i64);

            let delayed_sample =
                self.sample_buffer[delayed_buffer_cursor as usize][self.current_channel];

            self.current_channel += 1;

            if self.current_channel >= self.channels {
                //Advance cursor
                self.buffer_cursor += 1;
                self.current_channel = 0;
            }

            if self.buffer_cursor >= self.sample_buffer.len() {
                self.buffer_cursor = 0;
            }

            Some(Sample::lerp(sample, delayed_sample, 1, 2))
        } else {
            None
        }
    }
}

impl<I, D> Source for Flanger<I, D>
where
    I: Source,
    I::Item: Sample,
    D: FromSample<I::Item> + Sample,
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

//new source, (start,end,buffered original?, effect source)

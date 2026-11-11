use rodio::{ChannelCount, SampleRate};
use rodio::{Sample, Source};

use super::mix_source::MixSource;

pub struct BitCrush<I: Source> {
    input: I,
    samples: u32,
    hold: Vec<I::Item>,
    mix: f32,
    sample_counter: u32,
    current_channel: u16,
    channels: ChannelCount,
}

pub fn bit_crusher<I: Source>(input: I, samples: u32) -> BitCrush<I> {
    let channels = input.channels();
    BitCrush {
        input,
        samples,
        hold: vec![I::Item::default(); channels.get() as usize],
        mix: 0.8,
        sample_counter: 0,
        current_channel: 0,
        channels,
    }
}

impl<I> Iterator for BitCrush<I>
where
    I: Source,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let source = self.input.next()?;

        if self.mix < f32::EPSILON {
            return Some(source);
        }

        if self.sample_counter == 0 {
            self.hold[self.current_channel as usize] = source;
        }

        let crushed = self.hold[self.current_channel as usize];

        self.current_channel += 1;

        if self.current_channel >= self.channels.get() {
            self.sample_counter += 1;

            if self.sample_counter >= self.samples {
                self.sample_counter = 0;
            }

            self.current_channel = 0;
        }

        Some(crate::lerp(
            source,
            crushed,
            (self.mix * 1000.0) as u32,
            1000,
        ))
    }
}

impl<I> Source for BitCrush<I>
where
    I: Source,
{
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        self.input.total_duration()
    }
}

impl<I> MixSource for BitCrush<I>
where
    I: Source,
{
    fn set_mix(&mut self, mix: f32) {
        self.mix = mix;
    }
}

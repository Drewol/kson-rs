use rodio::{Sample, Source};

use super::mix_source::MixSource;

pub struct BitCrush<I: Source<Item = D>, D: Sample> {
    input: I,
    samples: u32,
    hold: Vec<I::Item>,
    mix: f32,
    sample_counter: u32,
    current_channel: u16,
    channels: u16,
}

pub fn bit_crusher<I: Source<Item = D>, D: Sample>(input: I, samples: u32) -> BitCrush<I, D> {
    let channels = input.channels();
    BitCrush {
        input,
        samples,
        hold: vec![D::zero_value(); channels as usize],
        mix: 0.8,
        sample_counter: 0,
        current_channel: 0,
        channels,
    }
}

impl<I, D> Iterator for BitCrush<I, D>
where
    I: Source<Item = D>,
    D: Sample,
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

        if self.current_channel >= self.channels {
            self.sample_counter += 1;

            if self.sample_counter >= self.samples {
                self.sample_counter = 0;
            }

            self.current_channel = 0;
        }

        Some(Sample::lerp(
            source,
            crushed,
            (self.mix * 1000.0) as u32,
            1000,
        ))
    }
}

impl<I, D> Source for BitCrush<I, D>
where
    I: Source<Item = D>,
    D: Sample,
{
    fn current_frame_len(&self) -> Option<usize> {
        self.input.current_frame_len()
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        self.input.total_duration()
    }
}

impl<I, D> MixSource for BitCrush<I, D>
where
    I: Source<Item = D>,
    D: Sample,
{
    fn set_mix(&mut self, mix: f32) {
        self.mix = mix;
    }
}

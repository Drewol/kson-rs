use std::collections::VecDeque;

use rodio::Source;

use super::mix_source::MixSource;

pub fn pitch_shift<I: Source<Item = f32>>(mut input: I, semitones: i32) -> PitchShift<I> {
    PitchShift { input }
}

pub struct PitchShift<I: Source<Item = f32>> {
    input: I,
}

impl<I> Iterator for PitchShift<I>
where
    I: Source<Item = f32>,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.input.next()
    }
}

impl<I> Source for PitchShift<I>
where
    I: Source<Item = f32>,
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

    fn total_duration(&self) -> Option<std::time::Duration> {
        self.input.total_duration()
    }
}

impl<I> MixSource for PitchShift<I>
where
    I: Source<Item = f32>,
{
    fn set_mix(&mut self, mix: f32) {}
}

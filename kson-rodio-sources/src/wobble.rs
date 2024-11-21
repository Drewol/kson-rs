use std::f32::consts::SQRT_2;
use std::sync::mpsc::channel;

use super::biquad::{biquad, BiQuad, BiQuadState, BiQuadType, BiquadController};
use super::mix_source::MixSource;
use super::triangle::TriangleWave;
use rodio::source::UniformSourceIterator;
use rodio::Source;

pub struct Wobble<I: Source<Item = f32>> {
    input: BiQuad<I>,
    wobble: UniformSourceIterator<TriangleWave, f32>,
    f_min: f32,
    f_max: f32,
    update: u32,
    mix: f32,
    biquad_control: BiquadController,
}

pub fn wobble<I: Source<Item = f32>>(input: I, rate: f32, f_min: f32, f_max: f32) -> Wobble<I> {
    let wobble = UniformSourceIterator::new(
        TriangleWave::new(rate, 1.0, input.sample_rate(), 0.0),
        input.channels(),
        input.sample_rate(),
    );
    let (biquad_control, biquad_read) = channel();
    let input = biquad(
        input,
        BiQuadState::new(BiQuadType::LowPass, SQRT_2, f_min),
        Some(biquad_read),
    );

    Wobble {
        input,
        wobble,
        f_min,
        f_max,
        update: 999,
        mix: 0.5,
        biquad_control,
    }
}

impl<I> Iterator for Wobble<I>
where
    I: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        self.update += 1;
        let wobble_phase = self.wobble.next().unwrap_or_default().mul_add(0.5, 0.5);
        if self.update >= self.input.channels() as u32 * 10 {
            let freq = self.f_min * (self.f_max / self.f_min).powf(wobble_phase);

            _ = self.biquad_control.send((
                Some(BiQuadState::new(BiQuadType::LowPass, SQRT_2, freq)),
                Some(self.mix),
            ));
            self.update = 0;
        }

        self.input.next()
    }
}

impl<I> Source for Wobble<I>
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

impl<I> MixSource for Wobble<I>
where
    I: Source<Item = f32>,
{
    fn set_mix(&mut self, mix: f32) {
        self.mix = mix;
    }
}

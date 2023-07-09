use std::sync::mpsc::channel;

use rodio::{source::UniformSourceIterator, Source};

use super::{
    biquad::{biquad, BiQuadState, BiquadController},
    triangle::TriangleWave,
};

pub fn phaser(mut input: Box<dyn Source<Item = f32>>, stage: u32) -> Phaser {
    let stage = (stage.clamp(0, 12) * 2) / 2; //Clamp and make even
    let sample_rate = input.sample_rate();

    let mut stage_controls = vec![];
    input = Box::new(UniformSourceIterator::new(input, 2, sample_rate));
    for _ in 0..stage {
        let (controls, reader) = channel();
        input = Box::new(biquad(
            input,
            BiQuadState::new(
                super::biquad::BiQuadType::AllPass,
                std::f32::consts::SQRT_2,
                500.0,
            ),
            Some(reader),
        ));
        stage_controls.push(controls);
    }
    let f_min = 800.0f32;
    let f_max = 1000.0f32;

    let hi_cut_gain = -8.0f32;
    input = Box::new(biquad(
        input,
        BiQuadState::new(
            super::biquad::BiQuadType::HighShelf(hi_cut_gain),
            1.5,
            (f_min * f_max).sqrt(),
        ),
        None,
    ));

    Phaser {
        stage,
        input,
        stage_controls,
        channel: 0,
        phases: [
            TriangleWave::new(1.0, 0.5, sample_rate, 0.0),
            TriangleWave::new(1.0, 0.5, sample_rate, 1.0),
        ],
        feedback: 0.8,
        f_min,
        f_max,
        hi_cut_gain,
        za: [0.0; 2],
    }
}

pub struct Phaser {
    stage: u32,
    input: Box<dyn Source<Item = f32>>,
    stage_controls: Vec<BiquadController>,
    channel: usize,
    phases: [TriangleWave; 2],
    feedback: f32,
    f_min: f32,
    f_max: f32,
    hi_cut_gain: f32,
    za: [f32; 2],
}

impl Iterator for Phaser {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        self.input.next() //TODO
    }
}

impl Source for Phaser {
    fn current_frame_len(&self) -> Option<usize> {
        self.input.current_frame_len()
    }

    fn channels(&self) -> u16 {
        2
    }

    fn sample_rate(&self) -> u32 {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        self.input.total_duration()
    }
}

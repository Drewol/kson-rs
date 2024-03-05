use std::{
    f32::consts::SQRT_2,
    sync::mpsc::{Receiver, Sender},
};

use rodio::{Sample, Source};

use super::mix_source::MixSource;

#[derive(Debug, Copy, Clone)]
#[allow(unused)]
pub enum BiQuadType {
    Peaking(f32),
    AllPass,
    LowPass,
    HighPass,
    HighShelf(f32),
}

#[derive(Debug, Clone, Copy)]
pub struct BiQuadState {
    filter_type: BiQuadType,
    q: f32,
    freq: f32,
}

impl Default for BiQuadState {
    fn default() -> Self {
        Self {
            filter_type: BiQuadType::AllPass,
            q: SQRT_2,
            freq: 100.0,
        }
    }
}

impl BiQuadState {
    pub fn new(filter: BiQuadType, q: f32, freq: f32) -> Self {
        BiQuadState {
            filter_type: filter,
            q,
            freq,
        }
    }
}

pub type BiquadController = Sender<(Option<BiQuadState>, Option<f32>)>;

pub fn biquad<I: Source<Item = f32>>(
    input: I,
    state: BiQuadState,
    updater: Option<Receiver<(Option<BiQuadState>, Option<f32>)>>,
) -> BiQuad<I> {
    let channels = input.channels();
    let mut res = BiQuad {
        input,
        channels,
        mix: 1.0,
        a0: 0.0,
        a1: 0.0,
        a2: 0.0,
        b0: 0.0,
        b1: 0.0,
        b2: 0.0,
        za: vec![[0.0; 2]; channels as usize],
        zb: vec![[0.0; 2]; channels as usize],
        current_channel: 0,
        updater,
        state,
    };

    res.update(state);
    res
}

pub struct BiQuad<I: Source<Item = f32>> {
    a0: f32,
    a1: f32,
    a2: f32,
    b0: f32,
    b1: f32,
    b2: f32,
    za: Vec<[f32; 2]>,
    zb: Vec<[f32; 2]>,
    mix: f32,
    input: I,
    current_channel: u16,
    channels: u16,
    updater: Option<Receiver<(Option<BiQuadState>, Option<f32>)>>,
    state: BiQuadState,
}

impl<I: Source<Item = f32>> BiQuad<I> {
    fn set_peaking(&mut self, gain: f32) {
        let w0 = (2.0 * std::f32::consts::PI * self.state.freq) / self.input.sample_rate() as f32;
        let cw0 = w0.cos();
        let alpha = w0.sin() / (2.0 * self.state.q);
        let a = 10.0_f32.powf(gain / 40.0);

        self.b0 = 1.0 + (alpha * a);
        self.b1 = -2.0 * cw0;
        self.b2 = 1.0 - (alpha * a);
        self.a0 = 1.0 + (alpha / a);
        self.a1 = -2.0 * cw0;
        self.a2 = 1.0 - (alpha / a);
    }

    fn set_allpass(&mut self) {
        let w0 = (2.0 * std::f32::consts::PI * self.state.freq) / self.input.sample_rate() as f32;
        let cw0 = w0.cos();
        let alpha = w0.sin() / (2.0 * self.state.q);

        self.b0 = 1.0 - alpha;
        self.b1 = -2.0 * cw0;
        self.b2 = 1.0 + alpha;
        self.a0 = 1.0 + alpha;
        self.a1 = -2.0 * cw0;
        self.a2 = 1.0 - alpha;
    }

    fn set_lowpass(&mut self) {
        let w0 = (2.0 * std::f32::consts::PI * self.state.freq) / self.input.sample_rate() as f32;
        let cw0 = w0.cos();
        let alpha = w0.sin() / (2.0 * self.state.q);

        self.b0 = (1.0 - cw0) / 2.0;
        self.b1 = 1.0 - cw0;
        self.b2 = (1.0 - cw0) / 2.0;
        self.a0 = 1.0 + alpha;
        self.a1 = -2.0 * cw0;
        self.a2 = 1.0 - alpha;
    }

    fn set_highpass(&mut self) {
        let w0 = (2.0 * std::f32::consts::PI * self.state.freq) / self.input.sample_rate() as f32;
        let cw0 = w0.cos();
        let alpha = w0.sin() / (2.0 * self.state.q);

        self.b0 = (1.0 + cw0) / 2.0;
        self.b1 = -(1.0 + cw0);
        self.b2 = (1.0 + cw0) / 2.0;
        self.a0 = 1.0 + alpha;
        self.a1 = -2.0 * cw0;
        self.a2 = 1.0 - alpha;
    }

    fn set_high_shelf(&mut self, gain: f32) {
        let w0 = (2.0 * std::f32::consts::PI * self.state.freq) / self.input.sample_rate() as f32;
        let cw0 = w0.cos();
        let alpha = w0.sin() / (2.0 * self.state.q);
        let a = 10.0_f32.powf(gain / 40.0);
        let two_sqrt_aalpha = 2.0 * a.sqrt() * alpha;

        self.b0 = a * ((a + 1.0) + (a - 1.0) * cw0 + two_sqrt_aalpha);
        self.b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cw0);
        self.b2 = a * ((a + 1.0) + (a - 1.0) * cw0 - two_sqrt_aalpha);
        self.a0 = (a + 1.0) - (a - 1.0) * cw0 + two_sqrt_aalpha;
        self.a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cw0);
        self.a2 = (a + 1.0) - (a - 1.0) * cw0 - two_sqrt_aalpha;
    }

    pub fn update(&mut self, filter: BiQuadState) {
        //reset filter on large jumps
        if (self.state.freq - filter.freq).abs() > 1000.0 {
            self.za.iter_mut().for_each(|x| *x = [0.0, 0.0]);
            self.zb.iter_mut().for_each(|x| *x = [0.0, 0.0]);
        }

        self.state = filter;
        self.state.q = self.state.q.max(0.01);

        match filter.filter_type {
            BiQuadType::Peaking(gain) => self.set_peaking(gain),
            BiQuadType::LowPass => self.set_lowpass(),
            BiQuadType::HighPass => self.set_highpass(),
            BiQuadType::AllPass => self.set_allpass(),
            BiQuadType::HighShelf(gain) => self.set_high_shelf(gain),
        }
    }

    pub fn set_mix(&mut self, factor: f32) {
        self.mix = factor;
    }

    fn process(&mut self, sample: f32) -> f32 {
        let c = self.current_channel as usize;
        self.current_channel += 1;
        if self.current_channel >= self.channels {
            self.current_channel = 0;
            while let Some((filter, mix)) = self.updater.as_ref().and_then(|x| x.try_recv().ok()) {
                if let Some(filter) = filter {
                    self.update(filter);
                }
                if let Some(mix) = mix {
                    self.set_mix(mix);
                }
            }
        }

        let src = sample;
        let za = &mut self.za;
        let zb = &mut self.zb;
        let a0 = self.a0;
        let a1 = self.a1;
        let a2 = self.a2;
        let b0 = self.b0;
        let b1 = self.b1;
        let b2 = self.b2;

        let filtered = (b0 / a0) * src + (b1 / a0) * zb[c][0] + (b2 / a0) * zb[c][1]
            - (a1 / a0) * za[c][0]
            - (a2 / a0) * za[c][1];

        // Shift delay buffers
        zb[c][1] = zb[c][0];
        zb[c][0] = src;

        // Feedback the calculated value into the IIR delay buffers
        za[c][1] = za[c][0];
        za[c][0] = filtered;

        filtered * self.mix + src * (1.0 - self.mix)
    }
}

impl<I> Iterator for BiQuad<I>
where
    I: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        self.input.next().map(|s| self.process(s))
    }
}

impl<I> Source for BiQuad<I>
where
    I: Source<Item = f32>,
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

impl<I: Source<Item = f32>> MixSource for BiQuad<I> {
    fn set_mix(&mut self, mix: f32) {
        self.mix = mix;
    }
}

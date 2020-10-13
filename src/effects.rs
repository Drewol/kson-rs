#![allow(dead_code)]
use crate::parameter::{BoolParameter, EffectParameter};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::f32;

pub struct ReTrigger {
    update_period: EffectParameter<f32>,
    update_period_tempo_sync: BoolParameter,
    wave_length: EffectParameter<f32>,
    wave_length_tempo_sync: BoolParameter,
    rate: EffectParameter<f32>,
    update_trigger: BoolParameter,
    mix: EffectParameter<f32>,
}

pub struct Gate {
    wave_length: EffectParameter<f32>,
    wave_length_tempo_sync: BoolParameter,
    rate: EffectParameter<f32>,
    mix: EffectParameter<f32>,
}

pub struct Flanger {
    period: EffectParameter<f32>,
    period_tempo_sync: BoolParameter,
    delay: EffectParameter<i64>,
    depth: EffectParameter<i64>,
    feedback: EffectParameter<f32>,
    stereo_width: EffectParameter<f32>,
    vol: EffectParameter<f32>,
    mix: EffectParameter<f32>,
}

pub struct PitchShift {
    pitch: EffectParameter<f32>,
    pitch_quantize: BoolParameter,
    chunk_size: EffectParameter<i64>,
    overlap: EffectParameter<f32>,
    mix: EffectParameter<f32>,
}

pub struct BitCrusher {
    reduction: EffectParameter<i64>,
    mix: EffectParameter<f32>,
}

pub struct Phaser {
    period: EffectParameter<f32>,
    period_tempo_sync: BoolParameter,
    stage: EffectParameter<i64>,
    lo_freq: EffectParameter<f32>,
    hi_freq: EffectParameter<f32>,
    q: EffectParameter<f32>,
    feedback: EffectParameter<f32>,
    stereo_width: EffectParameter<f32>,
    mix: EffectParameter<f32>,
}

pub struct Wobble {
    wave_length: EffectParameter<f32>,
    wave_length_tempo_sync: BoolParameter,
    lo_freq: EffectParameter<f32>,
    hi_freq: EffectParameter<f32>,
    q: EffectParameter<f32>,
    mix: EffectParameter<f32>,
}

pub struct TapeStop {
    speed: EffectParameter<f32>,
    trigger: BoolParameter,
    mix: EffectParameter<f32>,
}

pub struct Echo {
    update_period: EffectParameter<f32>,
    update_period_tempo_sync: BoolParameter,
    wave_length: EffectParameter<f32>,
    wave_length_tempo_sync: BoolParameter,
    update_trigger: BoolParameter,
    feedback_level: EffectParameter<f32>,
    mix: EffectParameter<f32>,
}

pub struct SideChain {
    period: EffectParameter<f32>,
    period_tempo_sync: BoolParameter,
    hold_time: EffectParameter<f32>,
    hold_time_tempo_sync: BoolParameter,
    attack_time: EffectParameter<f32>,
    attack_time_tempo_sync: BoolParameter,
    release_time: EffectParameter<f32>,
    release_time_tempo_sync: BoolParameter,
    ratio: EffectParameter<f32>,
}

pub struct AudioSwap;

#[derive(Copy, Clone)]
pub enum BiQuadType {
    Peaking(f32),
    LowPass,
    HighPass,
}

impl Default for BiQuadType {
    fn default() -> BiQuadType {
        let two: f32 = 2.0;
        BiQuadType::Peaking(two.sqrt())
    }
}

#[derive(Default)]
pub struct BiQuad {
    a0: f32,
    a1: f32,
    a2: f32,
    b0: f32,
    b1: f32,
    b2: f32,
    za: Vec<[f32; 2]>,
    zb: Vec<[f32; 2]>,
    q: f32,
    rate: u32,
    mix: f32,
}

pub struct HighPassFilter {
    env: EffectParameter<f32>,
    lo_freq: EffectParameter<f32>,
    hi_freq: EffectParameter<f32>,
    q: EffectParameter<f32>,
    delay: EffectParameter<f32>,
    mix: EffectParameter<f32>,
    filter: BiQuad,
}

pub struct LowPassFilter {
    env: EffectParameter<f32>,
    lo_freq: EffectParameter<f32>,
    hi_freq: EffectParameter<f32>,
    q: EffectParameter<f32>,
    delay: EffectParameter<f32>,
    mix: EffectParameter<f32>,
    filter: BiQuad,
}

#[derive(Deserialize, Serialize)]
pub struct PeakingFilter {
    env: EffectParameter<f32>,
    lo_freq: EffectParameter<f32>,
    hi_freq: EffectParameter<f32>,
    q: EffectParameter<f32>,
    delay: EffectParameter<f32>,
    mix: EffectParameter<f32>,
    #[serde(skip_deserializing, skip_serializing)]
    filter: BiQuad,
}

pub trait DSP: Send + Sync {
    fn process(&mut self, sample: &mut f32, c: usize);
    fn set_param_transition(&mut self, v: f32, on: bool);
    fn update_params(&mut self, v: &Value);
}

impl BiQuad {
    fn set_peaking(&mut self, freq: f32, gain: f32) {
        let w0 = (2.0 * std::f32::consts::PI * freq) / self.rate as f32;
        let cw0 = w0.cos();
        let alpha = w0.sin() / (2.0 * self.q);
        let a = 10.0_f32.powf(gain / 40.0);

        self.b0 = 1.0 + (alpha * a);
        self.b1 = -2.0 * cw0;
        self.b2 = 1.0 - (alpha * a);
        self.a0 = 1.0 + (alpha / a);
        self.a1 = -2.0 * cw0;
        self.a2 = 1.0 - (alpha / a);
    }

    fn set_lowpass(&mut self, freq: f32) {
        let w0 = (2.0 * std::f32::consts::PI * freq) / self.rate as f32;
        let cw0 = w0.cos();
        let alpha = w0.sin() / (2.0 * self.q);

        self.b0 = (1.0 - cw0) / 2.0;
        self.b1 = 1.0 - cw0;
        self.b2 = (1.0 - cw0) / 2.0;
        self.a0 = 1.0 + alpha;
        self.a1 = -2.0 * cw0;
        self.a2 = 1.0 - alpha;
    }

    fn set_highpass(&mut self, freq: f32) {
        let w0 = (2.0 * std::f32::consts::PI * freq) / self.rate as f32;
        let cw0 = w0.cos();
        let alpha = w0.sin() / (2.0 * self.q);

        self.b0 = (1.0 + cw0) / 2.0;
        self.b1 = -(1.0 + cw0);
        self.b2 = (1.0 + cw0) / 2.0;
        self.a0 = 1.0 + alpha;
        self.a1 = -2.0 * cw0;
        self.a2 = 1.0 - alpha;
    }

    pub fn new(filter_type: BiQuadType, rate: u32, f0: f32, q: f32, channels: usize) -> Self {
        let mut filter = BiQuad {
            a0: 0.0,
            a1: 0.0,
            a2: 0.0,
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            za: Vec::new(),
            zb: Vec::new(),
            rate,
            q: q.max(0.01),
            mix: 1.0,
        };

        for _ in 0..channels {
            filter.za.push([0.0, 0.0]);
            filter.zb.push([0.0, 0.0]);
        }

        match filter_type {
            BiQuadType::HighPass => filter.set_highpass(f0),
            BiQuadType::LowPass => filter.set_lowpass(f0),
            BiQuadType::Peaking(gain) => filter.set_peaking(f0, gain),
        }

        filter
    }

    fn process(&mut self, sample: &mut f32, c: usize) {
        let src = *sample;
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

        *sample = filtered * self.mix + src * (1.0 - self.mix);
    }
}

impl DSP for PeakingFilter {
    fn process(&mut self, sample: &mut f32, c: usize) {
        self.filter.process(sample, c);
    }
    fn set_param_transition(&mut self, v: f32, on: bool) {
        self.env.v = self.env.interpolate(v, on);
        self.delay.v = self.delay.interpolate(v, on);
        self.hi_freq.v = self.hi_freq.interpolate(v, on);
        self.lo_freq.v = self.lo_freq.interpolate(v, on);
        self.mix.v = self.mix.interpolate(v, on);
        self.q.v = self.q.interpolate(v, on);

        let width = self.hi_freq.v - self.lo_freq.v;
        let freq = (self.lo_freq.v + width * v).exp();

        self.filter.set_peaking(freq, self.q.v);
    }
    fn update_params(&mut self, _v: &Value) {}
}

impl DSP for LowPassFilter {
    fn process(&mut self, sample: &mut f32, c: usize) {
        self.filter.process(sample, c);
    }
    fn set_param_transition(&mut self, v: f32, on: bool) {
        self.env.v = self.env.interpolate(v, on);
        self.delay.v = self.delay.interpolate(v, on);
        self.hi_freq.v = self.hi_freq.interpolate(v, on);
        self.lo_freq.v = self.lo_freq.interpolate(v, on);
        self.mix.v = self.mix.interpolate(v, on);
        self.q.v = self.q.interpolate(v, on);

        let width = self.hi_freq.v - self.lo_freq.v;
        let freq = (self.lo_freq.v + width * v).exp();
        self.filter.q = self.q.v;
        self.filter.set_lowpass(freq);
    }
    fn update_params(&mut self, _v: &Value) {}
}

impl DSP for HighPassFilter {
    fn process(&mut self, sample: &mut f32, c: usize) {
        self.filter.process(sample, c);
    }
    fn set_param_transition(&mut self, v: f32, on: bool) {
        self.env.v = self.env.interpolate(v, on);
        self.delay.v = self.delay.interpolate(v, on);
        self.hi_freq.v = self.hi_freq.interpolate(v, on);
        self.lo_freq.v = self.lo_freq.interpolate(v, on);
        self.mix.v = self.mix.interpolate(v, on);
        self.q.v = self.q.interpolate(v, on);

        let width = self.hi_freq.v - self.lo_freq.v;
        let freq = (self.lo_freq.v + width * v).exp();
        self.filter.q = self.q.v;
        self.filter.set_highpass(freq);
    }
    fn update_params(&mut self, _v: &Value) {}
}

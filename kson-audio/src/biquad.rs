use kson::parameter::Parameter;

use crate::Dsp;

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

pub(crate) struct PeakingInternal {
    params: kson::effects::PeakingFilter,
    filter: BiQuad,
}

impl PeakingInternal {
    pub(crate) fn new(params: kson::effects::PeakingFilter, filter: BiQuad) -> Self {
        Self { params, filter }
    }
}

pub(crate) struct LowPassInternal {
    params: kson::effects::LowPassFilter,
    filter: BiQuad,
}

impl LowPassInternal {
    pub(crate) fn new(params: kson::effects::LowPassFilter, filter: BiQuad) -> Self {
        Self { params, filter }
    }
}

pub(crate) struct HighPassInternal {
    params: kson::effects::HighPassFilter,
    filter: BiQuad,
}

impl HighPassInternal {
    pub(crate) fn new(params: kson::effects::HighPassFilter, filter: BiQuad) -> Self {
        Self { params, filter }
    }
}

#[derive(Default, Clone)]
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

impl Dsp for PeakingInternal {
    fn process(&mut self, sample: &mut f32, c: usize) {
        self.filter.process(sample, c);
    }
    fn set_param_transition(&mut self, v: f32, on: bool) {
        self.params.v.v = self.params.v.interpolate(v, on);
        self.params.delay.v = self.params.delay.interpolate(v, on);
        self.params.freq_max.v = self.params.freq_max.interpolate(v, on);
        self.params.freq.v = self.params.freq.interpolate(v, on);
        self.params.mix.v = self.params.mix.interpolate(v, on);
        self.params.q.v = self.params.q.interpolate(v, on);

        let width = self.params.freq_max.v - self.params.freq.v;
        let freq = (self.params.freq.v + width * v).exp();

        self.filter.set_peaking(freq, self.params.q.v);
    }
}

impl Dsp for LowPassInternal {
    fn process(&mut self, sample: &mut f32, c: usize) {
        self.filter.process(sample, c);
    }
    fn set_param_transition(&mut self, v: f32, on: bool) {
        self.params.v.v = self.params.v.interpolate(v, on);
        self.params.delay.v = self.params.delay.interpolate(v, on);
        self.params.freq_max.v = self.params.freq_max.interpolate(v, on);
        self.params.freq.v = self.params.freq.interpolate(v, on);
        self.params.mix.v = self.params.mix.interpolate(v, on);
        self.params.q.v = self.params.q.interpolate(v, on);

        let width = self.params.freq_max.v - self.params.freq.v;
        let freq = (self.params.freq.v + width * v).exp();
        self.filter.q = self.params.q.v;
        self.filter.set_lowpass(freq);
    }
}

impl Dsp for HighPassInternal {
    fn process(&mut self, sample: &mut f32, c: usize) {
        self.filter.process(sample, c);
    }
    fn set_param_transition(&mut self, v: f32, on: bool) {
        self.params.v.v = self.params.v.interpolate(v, on);
        self.params.delay.v = self.params.delay.interpolate(v, on);
        self.params.freq_max.v = self.params.freq_max.interpolate(v, on);
        self.params.freq.v = self.params.freq.interpolate(v, on);
        self.params.mix.v = self.params.mix.interpolate(v, on);
        self.params.q.v = self.params.q.interpolate(v, on);

        let width = self.params.freq_max.v - self.params.freq.v;
        let freq = (self.params.freq.v + width * v).exp();
        self.filter.q = self.params.q.v;
        self.filter.set_highpass(freq);
    }
}

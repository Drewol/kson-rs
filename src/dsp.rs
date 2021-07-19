pub trait Dsp: Send + Sync {
    fn process(&mut self, sample: &mut f32, c: usize);
    fn set_mix(&mut self, mix: f32);
    fn set_bypass(&mut self, bypass: bool);
    fn set_param_transition(&mut self, v: f32);
}

#[derive(Copy, Clone)]
pub enum BiQuadType {
    Peaking(f32),
    LowPass,
    HighPass,
}

pub struct BiQuad {
    a0: f32,
    a1: f32,
    a2: f32,
    b0: f32,
    b1: f32,
    b2: f32,
    za: Vec<[f32; 2]>,
    zb: Vec<[f32; 2]>,
    filter_type: BiQuadType,
    q: f32,
    rate: u32,
    freq_end: f32,
    freq_start: f32,
    mix: f32,
    bypass: bool,
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

    pub fn new(
        filter_type: BiQuadType,
        rate: u32,
        f0: f32,
        f1: f32,
        q: f32,
        channels: usize,
    ) -> Self {
        let mut filter = BiQuad {
            a0: 0.0,
            a1: 0.0,
            a2: 0.0,
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            za: Vec::new(),
            zb: Vec::new(),
            filter_type,
            rate,
            q: q.max(0.01),
            freq_start: f0,
            freq_end: f1,
            mix: 1.0,
            bypass: false,
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
}

impl Dsp for BiQuad {
    fn process(&mut self, sample: &mut f32, c: usize) {
        if self.bypass {
            return;
        }

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

    fn set_mix(&mut self, mix: f32) {
        self.mix = mix;
    }

    fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }

    fn set_param_transition(&mut self, v: f32) {
        let start = self.freq_start.ln();
        let end = self.freq_end.ln();

        let width = end - start;
        let freq = (start + width * v).exp();

        match self.filter_type {
            BiQuadType::HighPass => self.set_highpass(freq),
            BiQuadType::LowPass => self.set_lowpass(freq),
            BiQuadType::Peaking(gain) => self.set_peaking(freq, gain),
        }
    }
}

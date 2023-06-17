use std::sync::mpsc::Receiver;

use rodio::Source;

#[derive(Copy, Clone)]
pub enum BiQuadType {
    Peaking(f32, f32),
    LowPass(f32),
    HighPass(f32),
}

pub fn biquad<I: Source<Item = f32>>(
    input: I,
    filter: BiQuadType,
    updater: Option<Receiver<(Option<BiQuadType>, Option<f32>)>>,
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
        q: std::f32::consts::SQRT_2,
        current_channel: 0,
        updater,
    };

    res.update(filter);
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
    q: f32,
    mix: f32,
    input: I,
    current_channel: u16,
    channels: u16,
    updater: Option<Receiver<(Option<BiQuadType>, Option<f32>)>>,
}

impl<I: Source<Item = f32>> BiQuad<I> {
    fn set_peaking(&mut self, freq: f32, gain: f32) {
        let w0 = (2.0 * std::f32::consts::PI * freq) / self.input.sample_rate() as f32;
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
        let w0 = (2.0 * std::f32::consts::PI * freq) / self.input.sample_rate() as f32;
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
        let w0 = (2.0 * std::f32::consts::PI * freq) / self.input.sample_rate() as f32;
        let cw0 = w0.cos();
        let alpha = w0.sin() / (2.0 * self.q);

        self.b0 = (1.0 + cw0) / 2.0;
        self.b1 = -(1.0 + cw0);
        self.b2 = (1.0 + cw0) / 2.0;
        self.a0 = 1.0 + alpha;
        self.a1 = -2.0 * cw0;
        self.a2 = 1.0 - alpha;
    }

    pub fn update(&mut self, filter: BiQuadType) {
        match filter {
            BiQuadType::Peaking(freq, q) => self.set_peaking(freq, q),
            BiQuadType::LowPass(freq) => self.set_lowpass(freq),
            BiQuadType::HighPass(freq) => self.set_highpass(freq),
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

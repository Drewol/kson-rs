//! Phaser: a chain of all-pass filters whose center frequency is swept by an LFO,
//! blended with the original audio (spec: `phaser`).

use rodio::{ChannelCount, SampleRate, Source};
use std::time::Duration;

use super::mix_source::MixSource;

/// Frames between filter coefficient updates
const UPDATE_INTERVAL: u32 = 32;
/// Upper bound for the feedback rate to keep the loop stable
const MAX_FEEDBACK: f32 = 0.95;

#[derive(Debug, Clone, Copy)]
pub struct PhaserParams {
    /// Number of all-pass filters (0-12)
    pub stages: usize,
    /// LFO period; zero keeps the LFO phase at its current value
    pub period: Duration,
    /// LFO end frequencies in Hz. `freq_1` may exceed `freq_2`.
    pub freq_1: f32,
    pub freq_2: f32,
    /// Q of the all-pass filters
    pub q: f32,
    /// Feedback rate (0.0-1.0)
    pub feedback: f32,
    /// LFO phase difference between L/R (0.0-1.0, where 1.0 is half a cycle)
    pub stereo_width: f32,
    /// Gain in dB (<= 0) above the center of `freq_1` and `freq_2`
    pub hi_cut_gain: f32,
}

/// Direct form I biquad section with normalized coefficients
#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
}

impl Biquad {
    fn set(&mut self, b: [f32; 3], a: [f32; 3]) {
        self.b = [b[0] / a[0], b[1] / a[0], b[2] / a[0]];
        self.a = [a[1] / a[0], a[2] / a[0]];
    }

    /// Second-order all-pass (Audio EQ Cookbook)
    fn set_all_pass(&mut self, freq: f32, q: f32, sample_rate: f32) {
        let w0 = std::f32::consts::TAU * freq / sample_rate;
        let alpha = w0.sin() / (2.0 * q);
        let cos = w0.cos();
        self.set(
            [1.0 - alpha, -2.0 * cos, 1.0 + alpha],
            [1.0 + alpha, -2.0 * cos, 1.0 - alpha],
        );
    }

    /// High shelf with a shelf slope of 1 (Audio EQ Cookbook)
    fn set_high_shelf(&mut self, freq: f32, gain_db: f32, sample_rate: f32) {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = std::f32::consts::TAU * freq / sample_rate;
        let alpha = w0.sin() / std::f32::consts::SQRT_2;
        let cos = w0.cos();
        let k = 2.0 * a.sqrt() * alpha;
        self.set(
            [
                a * ((a + 1.0) + (a - 1.0) * cos + k),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - k),
            ],
            [
                (a + 1.0) - (a - 1.0) * cos + k,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - k,
            ],
        );
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

struct ChannelState {
    stages: Vec<Biquad>,
    hi_cut: Biquad,
    last_out: f32,
}

pub struct Phaser<I: Source<Item = f32>> {
    input: I,
    params: PhaserParams,
    channels: Vec<ChannelState>,
    current_channel: usize,
    /// LFO phase in cycles (0.0-1.0)
    phase: f32,
    phase_step: f32,
    frames_until_update: u32,
    mix: f32,
}

pub fn phaser<I: Source<Item = f32>>(input: I, params: PhaserParams) -> Phaser<I> {
    let sample_rate = input.sample_rate().get() as f32;
    let channel_count = input.channels().get() as usize;
    let nyquist_guard = sample_rate * 0.45;

    let mut params = params;
    params.stages = params.stages.min(12);
    params.freq_1 = params.freq_1.clamp(10.0, nyquist_guard);
    params.freq_2 = params.freq_2.clamp(10.0, nyquist_guard);
    params.q = params.q.max(0.1);
    params.feedback = params.feedback.clamp(0.0, MAX_FEEDBACK);
    params.hi_cut_gain = params.hi_cut_gain.min(0.0);

    let hi_cut_freq = (params.freq_1 * params.freq_2).sqrt();
    let channels = (0..channel_count)
        .map(|_| {
            let mut hi_cut = Biquad::default();
            hi_cut.set_high_shelf(hi_cut_freq, params.hi_cut_gain, sample_rate);
            ChannelState {
                stages: vec![Biquad::default(); params.stages],
                hi_cut,
                last_out: 0.0,
            }
        })
        .collect();

    let period = params.period.as_secs_f32();
    let mut phaser = Phaser {
        input,
        params,
        channels,
        current_channel: 0,
        phase: 0.0,
        phase_step: if period > 0.0 {
            1.0 / (period * sample_rate)
        } else {
            0.0
        },
        frames_until_update: 0,
        mix: 1.0,
    };
    phaser.update_coefficients();
    phaser
}

impl<I: Source<Item = f32>> Phaser<I> {
    /// Triangle LFO (0.0-1.0) mapped to a frequency on a log scale
    fn lfo_freq(&self, phase: f32) -> f32 {
        let tri = 1.0 - (2.0 * phase.rem_euclid(1.0) - 1.0).abs();
        self.params.freq_1 * (self.params.freq_2 / self.params.freq_1).powf(tri)
    }

    fn update_coefficients(&mut self) {
        let sample_rate = self.input.sample_rate().get() as f32;
        let q = self.params.q;
        for c in 0..self.channels.len() {
            // Odd channels are offset by up to half an LFO cycle
            let offset = if c % 2 == 1 {
                self.params.stereo_width * 0.5
            } else {
                0.0
            };
            let freq = self.lfo_freq(self.phase + offset);
            for stage in &mut self.channels[c].stages {
                stage.set_all_pass(freq, q, sample_rate);
            }
        }
    }
}

impl<I: Source<Item = f32>> Iterator for Phaser<I> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let x = self.input.next()?;
        let c = self.current_channel;
        self.current_channel += 1;
        if self.current_channel >= self.channels.len() {
            self.current_channel = 0;
            self.phase = (self.phase + self.phase_step) % 1.0;
            if self.frames_until_update == 0 {
                self.update_coefficients();
                self.frames_until_update = UPDATE_INTERVAL;
            }
            self.frames_until_update -= 1;
        }

        if self.mix < f32::EPSILON || self.params.stages == 0 {
            return Some(x);
        }

        let feedback = self.params.feedback;
        let state = &mut self.channels[c];
        let mut y = x + feedback * state.last_out;
        for stage in &mut state.stages {
            y = stage.process(y);
        }
        state.last_out = y;
        let wet = state.hi_cut.process(y);

        // The phaser mix value is halved when used, so 100% is a 50/50 blend (spec: phaser.mix)
        let mix = self.mix * 0.5;
        Some(x * (1.0 - mix) + wet * mix)
    }
}

impl<I: Source<Item = f32>> Source for Phaser<I> {
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.input.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }
}

impl<I: Source<Item = f32>> MixSource for Phaser<I> {
    fn set_mix(&mut self, mix: f32) {
        self.mix = mix;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::source::SineWave;

    fn params() -> PhaserParams {
        PhaserParams {
            stages: 6,
            period: Duration::from_millis(500),
            freq_1: 1500.0,
            freq_2: 20000.0,
            q: 0.707,
            feedback: 0.35,
            stereo_width: 0.75,
            hi_cut_gain: -8.0,
        }
    }

    #[test]
    fn output_is_finite_and_bounded() {
        let source = SineWave::new(440.0).take_duration(Duration::from_secs(2));
        let mut p = phaser(source, params());
        p.set_mix(1.0);
        let samples: Vec<f32> = p.collect();
        assert!(!samples.is_empty());
        assert!(samples.iter().all(|s| s.is_finite() && s.abs() < 4.0));
    }

    #[test]
    fn zero_mix_and_zero_stages_pass_through() {
        let dry: Vec<f32> = SineWave::new(440.0)
            .take_duration(Duration::from_millis(50))
            .collect();
        let mut p = phaser(
            SineWave::new(440.0).take_duration(Duration::from_millis(50)),
            params(),
        );
        p.set_mix(0.0);
        assert_eq!(p.collect::<Vec<_>>(), dry);

        let mut no_stages = params();
        no_stages.stages = 0;
        no_stages.period = Duration::ZERO;
        let mut p = phaser(
            SineWave::new(440.0).take_duration(Duration::from_millis(50)),
            no_stages,
        );
        p.set_mix(1.0);
        assert_eq!(p.collect::<Vec<_>>(), dry);
    }
}

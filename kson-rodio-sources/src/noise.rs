use rand::Rng;
use rodio::Source;
use rodio::{ChannelCount, SampleRate};

pub struct NoiseSource {
    sample_rate: SampleRate,
    amplitude: f32,
    rng: rand::rngs::OsRng,
    channels: ChannelCount,
}

impl NoiseSource {
    pub fn new(sample_rate: SampleRate, amplitude: f32, channels: ChannelCount) -> Self {
        NoiseSource {
            sample_rate,
            amplitude,
            rng: rand::rngs::OsRng,
            channels,
        }
    }
}

impl Iterator for NoiseSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        Some(self.rng.gen_range(-1.0..1.0) * self.amplitude)
    }
}

impl Source for NoiseSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

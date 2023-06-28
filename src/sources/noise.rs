use rand::Rng;
use rodio::Source;

pub struct NoiseSource {
    sample_rate: u32,
    amplitude: f32,
    rng: rand::rngs::OsRng,
    channels: u16,
}

impl NoiseSource {
    pub fn new(sample_rate: u32, amplitude: f32, channels: u16) -> Self {
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
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

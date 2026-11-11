use rodio::{nz, ChannelCount, SampleRate};
use rodio::{Sample, Source};

pub struct TriangleWave {
    frequency: f32,
    amplitude: f32,
    sample_rate: SampleRate,
    phase: f32,
}

impl TriangleWave {
    pub fn new(frequency: f32, amplitude: f32, sample_rate: SampleRate, phase: f32) -> Self {
        Self {
            frequency,
            amplitude,
            sample_rate,
            phase,
        }
    }
}

impl Iterator for TriangleWave {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let phase_increment = 2.0 * self.frequency / self.sample_rate.get() as f32;
        self.phase = (self.phase + phase_increment) % 2.0;

        Some(2.0 * self.amplitude * (self.phase - 1.0).abs() - self.amplitude)
    }
}

impl Source for TriangleWave {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        nz!(1)
    }

    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

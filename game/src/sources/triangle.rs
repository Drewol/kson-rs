use rodio::source::Source;

pub struct TriangleWave {
    frequency: f32,
    amplitude: f32,
    sample_rate: u32,
    phase: f32,
}

impl TriangleWave {
    pub fn new(frequency: f32, amplitude: f32, sample_rate: u32, phase: f32) -> Self {
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
        let phase_increment = 2.0 * self.frequency / self.sample_rate as f32;
        self.phase = (self.phase + phase_increment) % 2.0;

        Some(2.0 * self.amplitude * (self.phase - 1.0).abs() - self.amplitude)
    }
}

impl Source for TriangleWave {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        1
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

use kson::MeasureBeatLines;

pub struct BeatPulser {
    beat_bounds: (f64, f64),
    beat_iter: MeasureBeatLines,
    timing: (f32, f32, f32),
}

impl BeatPulser {
    pub fn new(chart: &kson::Chart) -> Self {
        let mut beat_iter = chart.beat_line_iter();
        let end_bound = chart.tick_to_ms(beat_iter.next().map(|x| x.0).unwrap_or(u32::MAX));

        Self {
            beat_bounds: (0.0, end_bound),
            timing: (0.0, 0.0, 0.0),
            beat_iter,
        }
    }

    pub fn update(
        &mut self,
        chart: &kson::Chart,
        chart_time: f64,
        speed_mult: f32,
        bpm: f64,
        dt: f64,
    ) -> (f32, f32, f32) {
        while chart_time > self.beat_bounds.1 {
            self.beat_bounds.0 = self.beat_bounds.1;
            self.beat_bounds.1 =
                chart.tick_to_ms(self.beat_iter.next().map(|x| x.0).unwrap_or(u32::MAX));
        }

        self.timing.0 = ((chart_time - self.beat_bounds.0)
            / (self.beat_bounds.1 - self.beat_bounds.0))
            .clamp(0.0, 1.0) as f32;
        self.timing.1 += speed_mult * (dt / kson::beat_in_ms(bpm)) as f32;
        self.timing.2 = chart_time as f32 / 1000.0;

        self.timing
    }

    pub fn get(&self) -> (f32, f32, f32) {
        self.timing
    }
}

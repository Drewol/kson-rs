mod biquad;

pub trait Dsp: Send + Sync {
    fn process(&mut self, sample: &mut f32, c: usize);
    fn set_param_transition(&mut self, v: f32, on: bool);
    fn update_params(&mut self, v: &Self);
}

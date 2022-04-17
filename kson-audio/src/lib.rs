pub mod biquad;

pub trait Dsp: Send + Sync {
    fn process(&mut self, sample: &mut f32, c: usize);
    fn set_param_transition(&mut self, v: f32, on: bool);
}

pub fn dsp_from_definition(def: kson::effects::AudioEffect) -> Box<dyn Dsp> {
    match def {
        kson::effects::AudioEffect::HighPassFilter(hpf) => {
            Box::new(biquad::HighPassInternal::new(hpf, Default::default()))
        }
        kson::effects::AudioEffect::LowPassFilter(lpf) => {
            Box::new(biquad::LowPassInternal::new(lpf, Default::default()))
        }
        kson::effects::AudioEffect::PeakingFilter(peaking) => {
            Box::new(biquad::PeakingInternal::new(peaking, Default::default()))
        }
        _ => todo!(),
    }
}

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub enum InterpolationShape {
    Linear,
    Logarithmic,
    Smooth,
}

#[derive(Deserialize, Serialize)]
pub struct EffectParameter<T> {
    off: Option<T>,
    min: T,
    max: Option<T>,
    shape: InterpolationShape,
    #[serde(skip_deserializing, skip_serializing)]
    pub v: T,
}

// impl EffectParameter<bool> {
//     pub fn interpolate(&self, p: f32, on: bool) -> bool {
//         if on {
//             if p >= 0.5 {
//                 match self.max {
//                     Some(v) => v,
//                     None => self.min,
//                 }
//             } else {
//                 self.min
//             }
//         } else {
//             match self.off {
//                 Some(v) => v,
//                 None => self.min,
//             }
//         }
//     }
// }

impl EffectParameter<f32> {
    pub fn interpolate(&self, p: f32, on: bool) -> f32 {
        if on {
            match self.max {
                None => self.min,
                Some(m) => match self.shape {
                    InterpolationShape::Logarithmic => {
                        let end: f32 = m.ln();
                        let start: f32 = self.min.ln();
                        let width: f32 = end - start;
                        (start + width * p).exp()
                    }
                    InterpolationShape::Linear => (self.min + (m - self.min)) * p,
                    InterpolationShape::Smooth => {
                        let smooth_p = p * p * p * (p * (p * 6.0 - 15.0) + 10.0);
                        (self.min + (m - self.min)) * smooth_p
                    }
                },
            }
        } else {
            match self.off {
                Some(v) => v,
                None => self.min,
            }
        }
    }
}

impl EffectParameter<i64> {
    pub fn interpolate(&self, p: f32, on: bool) -> i64 {
        if on {
            match self.max {
                None => self.min,
                Some(m) => match self.shape {
                    InterpolationShape::Logarithmic => {
                        let end: f32 = (m as f32).ln();
                        let start: f32 = (self.min as f32).ln();
                        let width: f32 = end - start;
                        (start + width * p).exp() as i64
                    }
                    InterpolationShape::Linear => ((self.min + (m - self.min)) as f32 * p) as i64,
                    InterpolationShape::Smooth => {
                        let smooth_p = p * p * p * (p * (p * 6.0 - 15.0) + 10.0);
                        ((self.min + (m - self.min)) as f32 * smooth_p) as i64
                    }
                },
            }
        } else {
            match self.off {
                Some(v) => v,
                None => self.min,
            }
        }
    }
}

pub trait Parameter<T> {
    fn interpolate(&self, p: f32, on: bool) -> T;
}

pub struct BoolParameter(EffectParameter<f32>);
impl Parameter<bool> for BoolParameter {
    fn interpolate(&self, p: f32, on: bool) -> bool {
        self.0.interpolate(p, on) > 0.0
    }
}

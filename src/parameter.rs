use num_traits::{NumCast, NumOps};
use serde::{Deserialize, Serialize};

#[cfg(feature = "schema")]
use schemars::JsonSchema;

#[derive(Serialize, Deserialize, Clone, Copy)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum InterpolationShape {
    Linear,
    Logarithmic,
    Smooth,
}

impl Default for InterpolationShape {
    fn default() -> Self {
        InterpolationShape::Linear
    }
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct EffectParameter<T> {
    pub off: Option<T>,
    pub min: Option<T>,
    pub max: Option<T>,
    #[serde(default)]
    pub shape: InterpolationShape,
    #[serde(skip_deserializing, skip_serializing)]
    pub v: T,
}

impl<T: Copy> From<T> for EffectParameter<T> {
    fn from(v: T) -> Self {
        EffectParameter {
            min: Some(v),
            v,
            max: None,
            off: None,
            shape: InterpolationShape::Linear,
        }
    }
}

pub trait Parameter<T>: Sized {
    fn interpolate(&self, p: f32, on: bool) -> T;
    fn update(&mut self, other: &Self);
}

pub trait DeriveParameter: Sized {
    fn derive(&self, other: &Self) -> Self;
}

impl<T> DeriveParameter for EffectParameter<T>
where
    EffectParameter<T>: Parameter<T> + Clone,
{
    fn derive(&self, other: &Self) -> Self {
        let mut new_param = self.clone();
        new_param.update(other);
        new_param
    }
}

impl<T> Parameter<T> for EffectParameter<T>
where
    T: NumCast + Copy + NumOps + Default,
{
    fn interpolate(&self, p: f32, on: bool) -> T {
        if on {
            match (self.min, self.max) {
                (Some(min), None) => min,
                (Some(min), Some(max)) => match self.shape {
                    InterpolationShape::Logarithmic => {
                        let end: f32 = max.to_f32().unwrap_or(1.0).ln();
                        let start: f32 = min.to_f32().unwrap_or(1.0).ln();
                        let width: f32 = end - start;
                        num_traits::cast((start + width * p).exp()).unwrap_or_default()
                    }
                    InterpolationShape::Linear => {
                        num_traits::cast((min + (max - min)).to_f32().unwrap_or(1.0) * p)
                            .unwrap_or_default()
                    }
                    InterpolationShape::Smooth => {
                        let smooth_p = p * p * p * (p * (p * 6.0 - 15.0) + 10.0);
                        num_traits::cast((min + (max - min)).to_f32().unwrap_or(1.0) * smooth_p)
                            .unwrap_or_default()
                    }
                },
                (None, _) => unreachable!(),
            }
        } else {
            match self.off {
                Some(v) => v,
                None => self.min.unwrap(),
            }
        }
    }

    fn update(&mut self, other: &Self) {
        if other.max.is_some() {
            self.max = other.max;
        }

        if other.min.is_some() {
            self.min = other.min;
        }

        if other.off.is_some() {
            self.off = other.off;
        }
    }
}

impl DeriveParameter for String {
    fn derive(&self, other: &Self) -> Self {
        other.clone()
    }
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BoolParameter(EffectParameter<f32>);

impl Parameter<bool> for BoolParameter {
    fn interpolate(&self, p: f32, on: bool) -> bool {
        self.0.interpolate(p, on) > 0.0
    }

    fn update(&mut self, other: &Self) {
        self.0.update(&other.0);
    }
}

impl DeriveParameter for BoolParameter {
    fn derive(&self, other: &Self) -> Self {
        Self(self.0.derive(&other.0))
    }
}

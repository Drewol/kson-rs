use std::ops::{Add, Mul, Sub};

use serde::{Deserialize, Serialize};

#[cfg(feature = "schema")]
use schemars::JsonSchema;

#[derive(Serialize, Deserialize, Clone)]
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

impl<T: Default> From<T> for EffectParameter<T> {
    fn from(v: T) -> Self {
        EffectParameter {
            min: Some(v),
            ..Default::default()
        }
    }
}

impl<T: From<f32> + Into<f32> + Sub<Output = T> + Mul<Output = T> + Add<Output = T> + Copy>
    EffectParameter<T>
{
    pub fn interpolate(&self, p: f32, on: bool) -> T {
        if on {
            match (self.min, self.max) {
                (Some(min), None) => min,
                (Some(min), Some(max)) => match self.shape {
                    InterpolationShape::Logarithmic => {
                        let end: f32 = max.into().ln();
                        let start: f32 = min.into().ln();
                        let width: f32 = end - start;
                        (start + width * p).exp().into()
                    }
                    InterpolationShape::Linear => ((min + (max - min)).into() * p).into(),
                    InterpolationShape::Smooth => {
                        let smooth_p = p * p * p * (p * (p * 6.0 - 15.0) + 10.0);
                        ((min + (max - min)).into() * smooth_p).into()
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

    pub fn update(&mut self, other: &Self) {
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

pub trait Parameter<T> {
    fn interpolate(&self, p: f32, on: bool) -> T;
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BoolParameter(EffectParameter<f32>);

impl Parameter<bool> for BoolParameter {
    fn interpolate(&self, p: f32, on: bool) -> bool {
        self.0.interpolate(p, on) > 0.0
    }
}

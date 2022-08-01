use std::{any::Any, default, fmt::Display, ops::RangeInclusive, str::FromStr};

use num_traits::{NumCast, NumOps};
use serde::{de::Visitor, Deserialize, Serialize};

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
#[derive(Clone)]
pub enum EffectParameterValue {
    Length(RangeInclusive<f32>, bool),
    Sample(RangeInclusive<i32>),
    Switch(RangeInclusive<bool>),
    Rate(RangeInclusive<f32>),
    Freq(RangeInclusive<f32>),
    Pitch(RangeInclusive<f32>),
    Int(RangeInclusive<i32>),
    Float(RangeInclusive<f32>),
    Filename(String),
    Undefined,
}

impl EffectParameterValue {
    pub fn interpolate(&self, v: f32, shape: InterpolationShape) -> f32 {
        match self {
            EffectParameterValue::Length(_, _) => todo!(),
            EffectParameterValue::Sample(_) => todo!(),
            EffectParameterValue::Switch(_) => todo!(),
            EffectParameterValue::Rate(_) => todo!(),
            EffectParameterValue::Freq(_) => todo!(),
            EffectParameterValue::Pitch(_) => todo!(),
            EffectParameterValue::Int(_) => todo!(),
            EffectParameterValue::Float(_) => todo!(),
            EffectParameterValue::Filename(_) => todo!(),
            EffectParameterValue::Undefined => todo!(),
        }
    }
}

#[derive(Clone, Default)]
pub struct EffectParameter<T> {
    on: EffectParameterValue,
    off: Option<EffectParameterValue>,
    pub v: T,
    pub shape: InterpolationShape,
}

pub type BoolParameter = EffectParameter<bool>;

impl<T: Any> Serialize for EffectParameter<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if let Some(off) = &self.off {
            serializer.collect_str(&format_args!("{}>{}", off, self.on))
        } else {
            serializer.collect_str(&self.on)
        }
    }
}

impl<'de, T: Default> Deserialize<'de> for EffectParameter<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct EffectParameterVisitor;
        impl<'de> Visitor<'de> for EffectParameterVisitor {
            type Value = (String, Option<String>);

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("String")
            }
        }

        let (a, b) = deserializer.deserialize_str(EffectParameterVisitor)?;

        Ok(Self {
            v: T::default(),
            on: a.parse().map_err(serde::de::Error::custom)?,
            off: b.and_then(|o| EffectParameterValue::from_str(&o).ok()),
            shape: InterpolationShape::Linear,
        })
    }
}

impl Default for EffectParameterValue {
    fn default() -> Self {
        Self::Undefined
    }
}

impl Display for EffectParameterValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            EffectParameterValue::Length(l, tempo) => {
                if *tempo {
                    serialize_range(l, |v| v.to_string())
                } else {
                    serialize_range(l, |v| v.to_string() + "ms")
                }
            }
            EffectParameterValue::Sample(s) => serialize_range(s, |v| v.to_string() + "samples"),
            EffectParameterValue::Switch(s) => serialize_range(s, |v| {
                if *v {
                    "On".to_string()
                } else {
                    "Off".to_string()
                }
            }),
            EffectParameterValue::Rate(r)
            | EffectParameterValue::Freq(r)
            | EffectParameterValue::Pitch(r)
            | EffectParameterValue::Float(r) => serialize_range(r, |v| v.to_string()),
            EffectParameterValue::Int(i) => serialize_range(i, i32::to_string),
            EffectParameterValue::Filename(f) => f.clone(),
            EffectParameterValue::Undefined => unreachable!(),
        };
        f.write_str(&str)
    }
}

impl<T> EffectParameter<T>
where
    T: NumCast + Copy + NumOps + Default,
{
    pub fn interpolate(&self, p: f32, on: bool) -> T {
        if on {
            T::from(self.on.interpolate(p, self.shape)).unwrap_or_default()
        } else {
            T::from(
                self.off
                    .as_ref()
                    .unwrap_or(&self.on)
                    .interpolate(p, self.shape),
            )
            .unwrap_or_default()
        }
    }
}

impl FromStr for EffectParameterValue {
    type Err = &'static str;

    fn from_str(v: &str) -> Result<Self, Self::Err> {
        let parse_part = |v: &str| {
            if v.contains('/') {
                if let Some((Ok(a), Ok(b))) = v
                    .split_once('/')
                    .map(|v| (v.0.parse::<f32>(), v.1.parse::<f32>()))
                {
                    let v = a / b;
                    return EffectParameterValue::Length(v..=v, true);
                }
            }

            if v.ends_with('%') {
                if let Ok(r) = v.parse::<f32>() {
                    let r = r / 100.0;
                    return EffectParameterValue::Rate(r..=r);
                }
            }

            todo!()
        };

        if v.contains('-') {
            // Range
            if let Some((a, b)) = v.split_once('-') {
                let parsed = match (parse_part(a), parse_part(b)) {
                    (EffectParameterValue::Length(a, ab), EffectParameterValue::Length(b, bb)) => {
                        EffectParameterValue::Length(*a.start()..=*b.end(), ab || bb)
                    }
                    (EffectParameterValue::Sample(a), EffectParameterValue::Sample(b)) => {
                        EffectParameterValue::Sample(*a.start()..=*b.end())
                    }

                    (EffectParameterValue::Switch(a), EffectParameterValue::Switch(b)) => {
                        EffectParameterValue::Switch(*a.start()..=*b.end())
                    }
                    (EffectParameterValue::Rate(a), EffectParameterValue::Rate(b)) => {
                        EffectParameterValue::Rate(*a.start()..=*b.end())
                    }
                    (EffectParameterValue::Freq(a), EffectParameterValue::Freq(b)) => {
                        EffectParameterValue::Freq(*a.start()..=*b.end())
                    }
                    (EffectParameterValue::Pitch(a), EffectParameterValue::Pitch(b)) => {
                        EffectParameterValue::Pitch(*a.start()..=*b.end())
                    }
                    (EffectParameterValue::Int(a), EffectParameterValue::Int(b)) => {
                        EffectParameterValue::Int(*a.start()..=*b.end())
                    }
                    (EffectParameterValue::Float(a), EffectParameterValue::Float(b)) => {
                        EffectParameterValue::Float(*a.start()..=*b.end())
                    }
                    _ => EffectParameterValue::Filename(v.to_string()),
                };
                return Ok(parsed);
            }
        }

        Ok(EffectParameterValue::Filename(v.to_string()))
    }
}

fn serialize_range<T: PartialOrd, F>(r: &RangeInclusive<T>, ser: F) -> String
where
    F: Fn(&T) -> String,
{
    if let Some(std::cmp::Ordering::Equal) = r.end().partial_cmp(r.start()) {
        ser(r.start())
    } else {
        format!("{}-{}", ser(r.start()), ser(r.end()))
    }
}

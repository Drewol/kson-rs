use std::{any::Any, fmt::Display, marker::PhantomData, ops::RangeInclusive, str::FromStr};

use num_traits::{NumCast, NumOps};
use serde::{de::Visitor, Deserialize, Serialize};

#[cfg(feature = "schema")]
use schemars::JsonSchema;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
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

#[derive(Clone, Copy, PartialEq, Debug, PartialOrd)]

pub enum EffectFloat {
    Float(f32),
    Fraction(i32, i32),
}

impl From<&EffectFloat> for f32 {
    fn from(val: &EffectFloat) -> Self {
        match val {
            EffectFloat::Float(f) => *f,
            EffectFloat::Fraction(a, b) => *a as f32 / *b as f32,
        }
    }
}

impl ToString for EffectFloat {
    fn to_string(&self) -> String {
        match self {
            EffectFloat::Float(f) => f.to_string(),
            EffectFloat::Fraction(a, b) => format!("{}/{}", a, b),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, PartialOrd)]

pub enum EffectFreq {
    Hz(i32),
    Khz(f32),
}

impl ToString for EffectFreq {
    fn to_string(&self) -> String {
        match self {
            EffectFreq::Hz(f) => f.to_string() + "Hz",
            EffectFreq::Khz(f) => f.to_string() + "kHz",
        }
    }
}

impl From<&EffectFreq> for f32 {
    fn from(val: &EffectFreq) -> Self {
        match val {
            EffectFreq::Hz(f) => *f as f32,
            EffectFreq::Khz(kf) => kf * 1000.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]

pub enum EffectParameterValue {
    Length(RangeInclusive<EffectFloat>, bool),
    Sample(RangeInclusive<i32>),
    Switch(RangeInclusive<bool>),
    Rate(RangeInclusive<f32>),
    Freq(RangeInclusive<EffectFreq>),
    Pitch(RangeInclusive<f32>),
    Int(RangeInclusive<i32>),
    Float(RangeInclusive<f32>),
    Filename(String),
    Undefined,
}

trait EffectParam {
    fn interpolate(&self, v: f32, shape: InterpolationShape) -> f32;
}

impl EffectParam for RangeInclusive<f32> {
    fn interpolate(&self, v: f32, shape: InterpolationShape) -> f32 {
        let w = self.end() - self.start();
        match shape {
            InterpolationShape::Linear => self.start() + w * v,
            InterpolationShape::Logarithmic => {
                let sln = self.start().ln();
                let wn = self.end().ln() - sln;
                (sln + wn * v).exp()
            }
            InterpolationShape::Smooth => {
                //https://en.wikipedia.org/wiki/Smoothstep
                self.start() + (v * v * v * (v * (v * 6.0 - 15.0) + 10.0)) * w
            }
        }
    }
}

impl EffectParam for RangeInclusive<EffectFloat> {
    fn interpolate(&self, v: f32, shape: InterpolationShape) -> f32 {
        RangeInclusive::<f32>::new(self.start().into(), self.end().into()).interpolate(v, shape)
    }
}

impl EffectParam for RangeInclusive<EffectFreq> {
    fn interpolate(&self, v: f32, shape: InterpolationShape) -> f32 {
        RangeInclusive::<f32>::new(self.start().into(), self.end().into()).interpolate(v, shape)
    }
}

impl EffectParam for RangeInclusive<i32> {
    fn interpolate(&self, v: f32, shape: InterpolationShape) -> f32 {
        ((*self.start() as f32)..=(*self.end() as f32)).interpolate(v, shape)
    }
}

impl EffectParam for EffectParameterValue {
    fn interpolate(&self, v: f32, shape: InterpolationShape) -> f32 {
        match self {
            EffectParameterValue::Length(a, _) => a.interpolate(v, shape),
            EffectParameterValue::Switch(s) => (if v <= 0.5 { s.start() } else { s.end() })
                .then(|| 1.0)
                .unwrap_or(0.0),
            EffectParameterValue::Rate(r)
            | EffectParameterValue::Pitch(r)
            | EffectParameterValue::Float(r) => r.interpolate(v, shape),
            EffectParameterValue::Freq(f) => f.interpolate(v, shape),
            EffectParameterValue::Sample(i) | EffectParameterValue::Int(i) => {
                i.interpolate(v, shape)
            }
            EffectParameterValue::Filename(_) => f32::NAN,
            EffectParameterValue::Undefined => f32::NAN,
        }
    }
}

#[derive(Clone, Default, Debug, PartialEq)]
pub struct EffectParameter<T> {
    pub off: EffectParameterValue,
    pub on: Option<EffectParameterValue>,
    pub v: T,
    pub shape: InterpolationShape,
}

pub type BoolParameter = EffectParameter<bool>;

impl<T: Any> Serialize for EffectParameter<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if let Some(on) = &self.on {
            serializer.collect_str(&format_args!("{}>{}", self.off, on))
        } else {
            serializer.collect_str(&self.off)
        }
    }
}

impl<T: Any> ToString for EffectParameter<T> {
    fn to_string(&self) -> String {
        if let Some(on) = &self.on {
            format!("{}>{}", self.off, on)
        } else {
            format!("{}", &self.off)
        }
    }
}

impl<'de, T: Default> Deserialize<'de> for EffectParameter<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct EffectParameterVisitor<T> {
            p: PhantomData<T>,
        }

        impl<'de, T: Default> Visitor<'de> for EffectParameterVisitor<T> {
            type Value = EffectParameter<T>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("String")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                v.parse().map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_str(EffectParameterVisitor::<T> { p: PhantomData })
    }
}

impl<T: Default> FromStr for EffectParameter<T> {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (a, b) = {
            let mut split = s.split('>');
            if let Some(a) = split.next() {
                Ok((a.to_string(), split.next().map(str::to_string)))
            } else {
                Err("Missing value")
            }
        }?;

        Ok(Self {
            v: T::default(),
            off: a.parse()?,
            on: b.and_then(|o| EffectParameterValue::from_str(&o).ok()),
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
                    "on".to_string()
                } else {
                    "off".to_string()
                }
            }),
            EffectParameterValue::Rate(r)
            | EffectParameterValue::Pitch(r)
            | EffectParameterValue::Float(r) => serialize_range(r, |v| v.to_string()),
            EffectParameterValue::Int(i) => serialize_range(i, i32::to_string),
            EffectParameterValue::Freq(f) => serialize_range(f, |v| v.to_string()),
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
            T::from(
                self.on
                    .as_ref()
                    .unwrap_or(&self.off)
                    .interpolate(p, self.shape),
            )
            .unwrap_or_default()
        } else {
            T::from(self.off.interpolate(p, self.shape)).unwrap_or_default()
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
                    .map(|v| (v.0.parse::<i32>(), v.1.parse::<i32>()))
                {
                    let v = EffectFloat::Fraction(a, b);
                    return EffectParameterValue::Length(v..=v, true);
                }
            }

            if v.ends_with("ms") {
                if let Ok(r) = v.trim_end_matches("ms").parse::<f32>() {
                    let r = EffectFloat::Float(r / 1000.0);
                    return EffectParameterValue::Length(r..=r, false);
                }
            }
            if v.ends_with('s') {
                if let Ok(r) = v.trim_end_matches('s').parse::<f32>() {
                    let r = EffectFloat::Float(r);
                    return EffectParameterValue::Length(r..=r, false);
                }
            }

            if v.ends_with('%') {
                if let Ok(r) = v.trim_end_matches('%').parse::<f32>() {
                    let r = r / 100.0;
                    return EffectParameterValue::Rate(r..=r);
                }
            }

            if v.ends_with("kHz") {
                if let Ok(r) = v.trim_end_matches("kHz").parse::<f32>() {
                    let r = EffectFreq::Khz(r);
                    return EffectParameterValue::Freq(r..=r);
                }
            }

            if v.ends_with("Hz") {
                if let Ok(r) = v.trim_end_matches("Hz").parse::<i32>() {
                    let r = EffectFreq::Hz(r);
                    return EffectParameterValue::Freq(r..=r);
                }
            }
            if v.ends_with("samples") {
                if let Ok(r) = v.trim_end_matches("samples").parse::<i32>() {
                    return EffectParameterValue::Sample(r..=r);
                }
            }

            if v.eq("on") {
                EffectParameterValue::Switch(true..=true)
            } else if v.eq("off") {
                EffectParameterValue::Switch(false..=false)
            } else {
                EffectParameterValue::Filename(v.to_string())
            }
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

        Ok(parse_part(v))
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

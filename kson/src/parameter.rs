use std::{
    any::Any, fmt::Display, marker::PhantomData, ops::RangeInclusive, str::FromStr, time::Duration,
};

use num_traits::{NumCast, NumOps};
use serde::{de::Visitor, Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub enum InterpolationShape {
    #[default]
    Linear,
    Logarithmic,
    Smooth,
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

impl Display for EffectFloat {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            EffectFloat::Float(f) => f.fmt(formatter),
            EffectFloat::Fraction(a, b) => formatter.write_fmt(format_args!("{}/{}", a, b)),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, PartialOrd)]

pub enum EffectFreq {
    Hz(i32),
    Khz(f32),
}

impl Display for EffectFreq {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            EffectFreq::Hz(f) => formatter.write_fmt(format_args!("{f}Hz")),
            EffectFreq::Khz(f) => formatter.write_fmt(format_args!("{f}kHz")),
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

#[derive(Clone, Debug, PartialEq, Default)]
pub enum EffectParameterValue {
    Length(RangeInclusive<EffectFloat>, bool),
    Sample(RangeInclusive<i32>),
    Switch(RangeInclusive<bool>),
    Rate(RangeInclusive<f32>),
    Freq(RangeInclusive<EffectFreq>),
    Pitch(RangeInclusive<f32>),
    Int(RangeInclusive<i32>),
    Float(RangeInclusive<f32>),
    Decibel(RangeInclusive<f32>),
    Filename(String),
    #[default]
    Undefined,
}

trait EffectParam {
    fn interpolate(&self, v: f32, shape: InterpolationShape) -> f32;
}

impl EffectParameterValue {
    pub fn default_shape(&self) -> InterpolationShape {
        match self {
            EffectParameterValue::Freq(_) => InterpolationShape::Logarithmic,
            _ => InterpolationShape::Linear,
        }
    }
    pub fn to_duration(&self, bpm: f32, v: f32) -> Duration {
        match self {
            EffectParameterValue::Length(l, tempo) => {
                if *tempo {
                    Duration::from_secs_f32(
                        (l.interpolate(v, InterpolationShape::Linear) * 240.0) / bpm,
                    )
                } else {
                    Duration::from_secs_f32(l.interpolate(v, InterpolationShape::Linear))
                }
            }
            EffectParameterValue::Sample(s) => {
                Duration::from_secs_f32(s.interpolate(v, InterpolationShape::Linear) / 44100.0)
            }
            EffectParameterValue::Rate(r) => Duration::from_secs_f32(
                (r.interpolate(v, InterpolationShape::Linear) * 240.0) / bpm,
            ),
            EffectParameterValue::Freq(f) => {
                Duration::from_secs_f32(1.0 / f.interpolate(v, InterpolationShape::Logarithmic))
            }
            EffectParameterValue::Int(_) => todo!(),
            EffectParameterValue::Float(f) => {
                Duration::from_secs_f32(f.interpolate(v, InterpolationShape::Linear))
            }
            EffectParameterValue::Switch(_) => Duration::ZERO,
            EffectParameterValue::Pitch(_) => Duration::ZERO,
            EffectParameterValue::Decibel(_) => Duration::ZERO,
            EffectParameterValue::Filename(_) => Duration::ZERO,
            EffectParameterValue::Undefined => Duration::ZERO,
        }
    }
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
            | EffectParameterValue::Float(r)
            | EffectParameterValue::Decibel(r) => r.interpolate(v, shape),
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
        serializer.collect_str(self)
    }
}

impl<T: Any> Display for EffectParameter<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self.on {
            Some(on) => formatter.write_fmt(format_args!("{}>{}", self.off, on)),
            None => formatter.write_fmt(format_args!("{}", &self.off)),
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

        impl<T: Default> Visitor<'_> for EffectParameterVisitor<T> {
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

    /// Parses one of the spec formats: `Off`, `Off>OnMin`, `OnMin-OnMax` or `Off>OnMin-OnMax`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (off, on) = match s.split_once('>') {
            Some((off, on)) => {
                let off: EffectParameterValue = off.parse()?;
                (off, EffectParameterValue::from_str(on).ok())
            }
            None => {
                let value: EffectParameterValue = s.parse()?;
                match value.range_start() {
                    // `OnMin-OnMax`: Off inherits the OnMin value
                    Some(on_min) if value.is_range() => (on_min, Some(value)),
                    _ => (value, None),
                }
            }
        };

        Ok(Self {
            v: T::default(),
            on,
            shape: off.default_shape(),
            off,
        })
    }
}

impl EffectParameterValue {
    fn is_range(&self) -> bool {
        fn r<T: PartialEq>(r: &RangeInclusive<T>) -> bool {
            r.start() != r.end()
        }
        match self {
            EffectParameterValue::Length(l, _) => r(l),
            EffectParameterValue::Sample(l) | EffectParameterValue::Int(l) => r(l),
            EffectParameterValue::Switch(l) => r(l),
            EffectParameterValue::Rate(l)
            | EffectParameterValue::Pitch(l)
            | EffectParameterValue::Float(l)
            | EffectParameterValue::Decibel(l) => r(l),
            EffectParameterValue::Freq(l) => r(l),
            EffectParameterValue::Filename(_) | EffectParameterValue::Undefined => false,
        }
    }

    /// Returns a single (non-range) value equal to the start of this value's range
    fn range_start(&self) -> Option<Self> {
        Some(match self {
            EffectParameterValue::Length(l, t) => EffectParameterValue::Length(*l.start()..=*l.start(), *t),
            EffectParameterValue::Sample(l) => EffectParameterValue::Sample(*l.start()..=*l.start()),
            EffectParameterValue::Int(l) => EffectParameterValue::Int(*l.start()..=*l.start()),
            EffectParameterValue::Switch(l) => EffectParameterValue::Switch(*l.start()..=*l.start()),
            EffectParameterValue::Rate(l) => EffectParameterValue::Rate(*l.start()..=*l.start()),
            EffectParameterValue::Pitch(l) => EffectParameterValue::Pitch(*l.start()..=*l.start()),
            EffectParameterValue::Float(l) => EffectParameterValue::Float(*l.start()..=*l.start()),
            EffectParameterValue::Decibel(l) => EffectParameterValue::Decibel(*l.start()..=*l.start()),
            EffectParameterValue::Freq(l) => EffectParameterValue::Freq(*l.start()..=*l.start()),
            EffectParameterValue::Filename(_) | EffectParameterValue::Undefined => return None,
        })
    }
}

impl Display for EffectParameterValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            EffectParameterValue::Length(l, tempo) => {
                if *tempo {
                    serialize_range(l, |v| v.to_string())
                } else {
                    serialize_range(l, |v| match v {
                        EffectFloat::Float(v) => (v * 1000.0).to_string() + "ms",
                        v => v.to_string(),
                    })
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
            EffectParameterValue::Rate(r) => serialize_range(r, |v| {
                // The spec only allows `[int]%`; use the `[float]` form otherwise
                let percent = v * 100.0;
                if (percent - percent.round()).abs() < 1e-3 {
                    format!("{}%", percent.round() as i32)
                } else {
                    v.to_string()
                }
            }),
            EffectParameterValue::Decibel(r) => serialize_range(r, |v| format!("{v}dB")),
            EffectParameterValue::Pitch(r) | EffectParameterValue::Float(r) => {
                serialize_range(r, |v| v.to_string())
            }
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

    pub fn to_duration(&self, bpm: f32, v: f32, on: bool) -> Duration {
        if on {
            let p = self.on.as_ref().unwrap_or(&self.off);

            p.to_duration(bpm, v)
        } else {
            self.off.to_duration(bpm, v)
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

            if v.ends_with("dB") {
                if let Ok(r) = v.trim_end_matches("dB").parse::<f32>() {
                    return EffectParameterValue::Decibel(r..=r);
                }
            }

            if v.ends_with('%') {
                if let Ok(r) = v.trim_end_matches('%').parse::<f32>() {
                    let r = r / 100.0;
                    return EffectParameterValue::Rate(r..=r);
                }
            }

            if v.ends_with("kHz") || v.ends_with("khz") {
                if let Ok(r) = v
                    .trim_end_matches("kHz")
                    .trim_end_matches("khz")
                    .parse::<f32>()
                {
                    let r = EffectFreq::Khz(r);
                    return EffectParameterValue::Freq(r..=r);
                }
            }

            if v.ends_with("Hz") || v.ends_with("hz") {
                if let Ok(r) = v
                    .trim_end_matches("Hz")
                    .trim_end_matches("hz")
                    .parse::<i32>()
                {
                    let r = EffectFreq::Hz(r);
                    return EffectParameterValue::Freq(r..=r);
                }
            }
            if v.ends_with("samples") {
                if let Ok(r) = v.trim_end_matches("samples").parse::<i32>() {
                    return EffectParameterValue::Sample(r..=r);
                }
            }

            if let Ok(v) = v.parse::<f32>() {
                return EffectParameterValue::Float(v..=v);
            }

            if let Ok(v) = v.parse::<i32>() {
                return EffectParameterValue::Float(v as f32..=v as f32);
            }

            if v.eq("on") {
                EffectParameterValue::Switch(true..=true)
            } else if v.eq("off") {
                EffectParameterValue::Switch(false..=false)
            } else {
                EffectParameterValue::Filename(v.to_string())
            }
        };

        // Range separator: the first '-' that is not a leading minus sign
        let separator = v
            .char_indices()
            .skip(1)
            .find(|(_, c)| *c == '-')
            .map(|(i, _)| i);
        if let Some(i) = separator {
            {
                let (a, b) = (&v[..i], &v[i + 1..]);
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
                    (EffectParameterValue::Decibel(a), EffectParameterValue::Decibel(b)) => {
                        EffectParameterValue::Decibel(*a.start()..=*b.end())
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

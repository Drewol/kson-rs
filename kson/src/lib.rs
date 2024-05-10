pub mod camera;
pub mod effects;
mod graph;
mod ksh;
pub mod overlaps;
pub mod parameter;
pub mod score_ticks;
mod vox;

use camera::CameraInfo;
use effects::AudioEffect;
pub use graph::*;
pub use ksh::*;
use serde::de::Visitor;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::marker::PhantomData;
use std::slice::Windows;
use std::str;
pub use vox::*;

type Dict<T> = HashMap<String, T>;

#[inline]
pub fn beat_in_ms(bpm: f64) -> f64 {
    60_000.0 / bpm
}

#[inline]
pub fn tick_in_ms(bpm: f64, ppqn: u32) -> f64 {
    beat_in_ms(bpm) / ppqn as f64
}

#[inline]
pub fn ticks_from_ms(ms: f64, bpm: f64, tpqn: u32) -> f64 {
    ms / tick_in_ms(bpm, tpqn)
}

#[inline]
pub fn ms_from_ticks(ticks: i64, bpm: f64, tpqn: u32) -> f64 {
    tick_in_ms(bpm, tpqn) * ticks as f64
}

#[repr(usize)]
#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum Side {
    Left = 0,
    Right,
}

impl Side {
    pub fn opposite(&self) -> Self {
        match self {
            Side::Left => Self::Right,
            Side::Right => Self::Left,
        }
    }
}

#[repr(usize)]
#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum BtLane {
    A = 0,
    B,
    C,
    D,
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub enum Track {
    BT(BtLane),
    FX(Side),
    Laser(Side),
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum SingleOrPair<T> {
    Single(T),
    Pair(T, T),
}

#[derive(Copy, Clone, Default)]
pub struct GraphPoint {
    pub y: u32,
    pub v: f64,
    pub vf: Option<f64>,
    pub a: Option<f64>,
    pub b: Option<f64>,
}
impl<'de> Deserialize<'de> for GraphPoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct GpVisitor;
        impl<'de> Visitor<'de> for GpVisitor {
            type Value = GraphPoint;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("[u32, f64 | [f64, f64], none | [f64, f64]]")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let y = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::custom("No element"))?;
                let (v, vf) = match seq
                    .next_element::<SingleOrPair<f64>>()?
                    .ok_or_else(|| serde::de::Error::custom("Missing 2nd element"))?
                {
                    SingleOrPair::Single(v) => (v, None),
                    SingleOrPair::Pair(v, vf) => (v, Some(vf)),
                };
                let (a, b) = if let Some((a, b)) = seq.next_element::<(f64, f64)>()? {
                    (Some(a), Some(b))
                } else {
                    (None, None)
                };

                Ok(GraphPoint { y, v, vf, a, b })
            }
        }

        deserializer.deserialize_seq(GpVisitor)
    }
}

impl Serialize for GraphPoint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeTuple;
        let point_len = if let (Some(a), Some(b)) = (self.a, self.b) {
            if (a - b).abs() > f64::EPSILON {
                3
            } else {
                2
            }
        } else {
            2
        };

        let mut top_tup = serializer.serialize_tuple(point_len)?;
        top_tup.serialize_element(&self.y)?;
        if let Some(vf) = self.vf {
            top_tup.serialize_element(&(self.v, vf))?;
        } else {
            top_tup.serialize_element(&self.v)?;
        }
        if point_len == 3 {
            top_tup.serialize_element(&(self.a.unwrap(), self.b.unwrap()))?;
        }

        top_tup.end()
    }
}

#[derive(Copy, Clone)]
pub struct GraphSectionPoint {
    pub ry: u32,
    pub v: f64,
    pub vf: Option<f64>,
    pub a: Option<f64>,
    pub b: Option<f64>,
}

impl<'de> Deserialize<'de> for GraphSectionPoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct GspVisitor;
        impl<'de> Visitor<'de> for GspVisitor {
            type Value = GraphSectionPoint;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("[u32, f64 | [f64, f64], none | [f64, f64]]")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let ry = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::custom("No element"))?;
                let (v, vf) = match seq
                    .next_element::<SingleOrPair<f64>>()?
                    .ok_or_else(|| serde::de::Error::custom("Missing 2nd element"))?
                {
                    SingleOrPair::Single(v) => (v, None),
                    SingleOrPair::Pair(v, vf) => (v, Some(vf)),
                };
                let (a, b) = if let Some((a, b)) = seq.next_element::<(f64, f64)>()? {
                    (Some(a), Some(b))
                } else {
                    (None, None)
                };

                Ok(GraphSectionPoint { ry, v, vf, a, b })
            }
        }

        deserializer.deserialize_seq(GspVisitor)
    }
}

impl Serialize for GraphSectionPoint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeTuple;
        let point_len = if let (Some(a), Some(b)) = (self.a, self.b) {
            if (a - b).abs() > f64::EPSILON {
                3
            } else {
                2
            }
        } else {
            2
        };

        let mut top_tup = serializer.serialize_tuple(point_len)?;
        top_tup.serialize_element(&self.ry)?;
        if let Some(vf) = self.vf {
            top_tup.serialize_element(&(self.v, vf))?;
        } else {
            top_tup.serialize_element(&self.v)?;
        }
        if point_len == 3 {
            top_tup.serialize_element(&(self.a.unwrap(), self.b.unwrap()))?;
        }

        top_tup.end()
    }
}

pub type ByMeasureIdx<T> = Vec<(u32, T)>;

impl GraphSectionPoint {
    pub fn new(ry: u32, v: f64) -> Self {
        GraphSectionPoint {
            ry,
            v,
            vf: None,
            a: None,
            b: None,
        }
    }
}

#[derive(Copy, Clone)]
pub struct Interval {
    pub y: u32,
    pub l: u32,
}

impl Serialize for Interval {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeTuple;
        if self.l == 0 {
            serializer.serialize_u32(self.y)
        } else {
            let mut tup = serializer.serialize_tuple(2)?;
            tup.serialize_element(&self.y)?;
            tup.serialize_element(&self.l)?;
            tup.end()
        }
    }
}

struct IntervalVisitor;

impl<'de> Visitor<'de> for IntervalVisitor {
    type Value = Interval;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("integer or `y` integer pair [`y`, `l`]")
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Interval { l: 0, y: v as u32 })
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Interval { l: 0, y: v as u32 })
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Interval { l: 0, y: v as u32 })
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let y = seq
            .next_element()?
            .ok_or_else(|| serde::de::Error::custom("Empty sequence"))?;
        let l = seq.next_element()?.unwrap_or(0);
        Ok(Interval { y, l })
    }
}

impl<'de> Deserialize<'de> for Interval {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(IntervalVisitor)
    }
}

fn default_zero<T: From<u8>>() -> T {
    T::from(0)
}

fn default_true<T: From<bool>>() -> T {
    T::from(true)
}

fn serde_eq<T: Into<i64> + Copy, const N: i64>(v: &T) -> bool {
    N == (*v).into()
}

#[allow(unused)]
fn serde_def_n<T: From<u32> + Copy, const N: u32>() -> T {
    N.into()
}

// fn default_false<T: From<bool>>() -> T {
//     T::from(false)
// }

/// (tick, section points, wide)
#[derive(Serialize, Deserialize, Clone)]
pub struct LaserSection(
    pub u32,
    pub Vec<GraphSectionPoint>,
    #[serde(
        default = "default_one::<u8>",
        skip_serializing_if = "serde_eq::<_, 1>"
    )]
    pub u8,
);

impl LaserSection {
    pub fn tick(&self) -> u32 {
        self.0
    }
    pub fn segments(&self) -> Windows<GraphSectionPoint> {
        self.1.windows(2)
    }

    pub fn last(&self) -> Option<&GraphSectionPoint> {
        self.1.last()
    }

    pub fn first(&self) -> Option<&GraphSectionPoint> {
        self.1.first()
    }

    pub fn wide(&self) -> u8 {
        self.2
    }
}

//https://github.com/m4saka/ksh2kson/issues/4#issuecomment-573343229
pub fn do_curve(x: f64, a: f64, b: f64) -> f64 {
    let t = if x < std::f64::EPSILON || a < std::f64::EPSILON {
        (a - (a * a + x - 2.0 * a * x).sqrt()) / (-1.0 + 2.0 * a)
    } else {
        x / (a + (a * a + (1.0 - 2.0 * a) * x).sqrt())
    };
    2.0 * (1.0 - t) * t * b + t * t
}

fn default_one<T: From<u8>>() -> T {
    T::from(1)
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NoteInfo {
    pub bt: [Vec<Interval>; 4],
    pub fx: [Vec<Interval>; 2],
    pub laser: [Vec<LaserSection>; 2],
}

impl NoteInfo {
    fn new() -> NoteInfo {
        NoteInfo {
            bt: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            fx: [Vec::new(), Vec::new()],
            laser: [Vec::new(), Vec::new()],
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DifficultyInfo {
    pub name: Option<String>,
    pub short_name: Option<String>,
    pub idx: u8,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MetaInfo {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_img_filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    pub artist: String,
    pub gauge: Option<GaugeInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist_img_filename: Option<String>,
    pub chart_author: String,
    pub difficulty: u8,
    pub level: u8,
    pub disp_bpm: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub std_bpm: Option<f64>,
    pub jacket_filename: String,
    pub jacket_author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub information: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct GaugeInfo {
    pub total: u32,
}

impl MetaInfo {
    fn new() -> MetaInfo {
        MetaInfo {
            title: String::new(),
            title_img_filename: None,
            subtitle: None,
            gauge: None,
            artist: String::new(),
            artist_img_filename: None,
            chart_author: String::new(),
            difficulty: 0,
            level: 1,
            disp_bpm: String::new(),
            std_bpm: None,
            jacket_filename: String::new(),
            jacket_author: String::new(),
            information: None,
        }
    }
}

pub type ByPulse<T> = Vec<(u32, T)>;
#[derive(Copy, Clone, Default)]
pub struct ByPulseOption<T>(u32, Option<T>);

impl<T> ByPulseOption<T> {
    pub fn tick(&self) -> u32 {
        self.0
    }

    pub fn value(&self) -> Option<&T> {
        self.1.as_ref()
    }

    pub fn new(y: u32, v: Option<T>) -> Self {
        Self(y, v)
    }
}

impl<T: Serialize> Serialize for ByPulseOption<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeTuple;
        if let Some(v) = &self.1 {
            let mut tup = serializer.serialize_tuple(2)?;
            tup.serialize_element(&self.0)?;
            tup.serialize_element(v)?;
            tup.end()
        } else {
            serializer.serialize_u32(self.0)
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for ByPulseOption<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ByPulseOptionVisitor<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for ByPulseOptionVisitor<T> {
            type Value = ByPulseOption<T>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("[`u32`, v] or `u32`")
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(ByPulseOption::<T>(v as u32, None))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(ByPulseOption::<T>(v as u32, None))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                Ok(ByPulseOption::<T>(
                    seq.next_element()?.unwrap_or_default(),
                    seq.next_element()?,
                ))
            }
        }

        deserializer.deserialize_any(ByPulseOptionVisitor(PhantomData))
    }
}

#[derive(Serialize, Deserialize, Copy, Clone)]
pub struct ByNote<T> {
    pub y: u32,
    pub v: Option<T>,
    #[serde(default = "default_true::<bool>")]
    pub dom: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ByNotes<T> {
    pub bt: Option<[Vec<ByNote<T>>; 4]>,
    pub fx: Option<[Vec<ByNote<T>>; 2]>,
    pub laser: Option<[Vec<ByNote<T>>; 2]>,
}

impl<'a, T> IntoIterator for &'a ByNotes<T> {
    type Item = (&'a ByNote<T>, Track);
    type IntoIter = ByNotesIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        ByNotesIter {
            by_notes: self,
            indexes: Default::default(),
        }
    }
}

pub struct ByNotesIter<'a, T> {
    by_notes: &'a ByNotes<T>,
    indexes: HashMap<Track, usize>,
}

impl<'a, T> Iterator for ByNotesIter<'a, T> {
    type Item = (&'a ByNote<T>, Track);

    fn next(&mut self) -> Option<Self::Item> {
        let mut current_events = HashMap::new();

        if let Some(bt) = &self.by_notes.bt {
            for (lane, bt) in bt.iter().enumerate() {
                let bt_lane = match lane {
                    0 => BtLane::A,
                    1 => BtLane::B,
                    2 => BtLane::C,
                    3 => BtLane::D,
                    _ => unreachable!(),
                };

                let track = Track::BT(bt_lane);
                let index = self.indexes.entry(track).or_insert(0);

                if let Some(note) = bt.get(*index) {
                    current_events.insert(track, note);
                }
            }
        }

        if let Some(fx) = &self.by_notes.fx {
            for (lane, fx) in fx.iter().enumerate() {
                let fx_lane = match lane {
                    0 => Side::Left,
                    1 => Side::Right,
                    _ => unreachable!(),
                };
                let track = Track::FX(fx_lane);
                let index = self.indexes.entry(track).or_insert(0);

                if let Some(note) = fx.get(*index) {
                    current_events.insert(track, note);
                }
            }
        }

        if let Some(laser) = &self.by_notes.laser {
            for (lane, laser) in laser.iter().enumerate() {
                let laser_lane = match lane {
                    0 => Side::Left,
                    1 => Side::Right,
                    _ => unreachable!(),
                };
                let track = Track::Laser(laser_lane);
                let index = self.indexes.entry(track).or_insert(0);

                if let Some(note) = laser.get(*index) {
                    current_events.insert(track, note);
                }
            }
        }

        if let Some((track, event)) = current_events.iter().min_by_key(|(_, evt)| evt.y) {
            self.indexes.entry(*track).and_modify(|i| *i += 1);
            Some((*event, *track))
        } else {
            None
        }
    }
}

/// (Numerator, Denominator)
#[derive(Serialize, Deserialize, Copy, Clone)]
pub struct TimeSignature(pub u32, pub u32);

impl TimeSignature {
    //Parse from "n/d" string
    fn from_str(s: &str) -> Self {
        let mut data = s.split('/');
        let n: u32 = data.next().unwrap_or("4").parse().unwrap_or(4);
        let d: u32 = data.next().unwrap_or("4").parse().unwrap_or(4);

        TimeSignature(n, d)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BeatInfo {
    pub bpm: ByPulse<f64>,
    pub time_sig: ByMeasureIdx<TimeSignature>,
    pub scroll_speed: Vec<GraphPoint>,
}

pub const KSON_RESOLUTION: u32 = 240;

impl BeatInfo {
    fn new() -> Self {
        BeatInfo {
            bpm: Vec::new(),
            time_sig: Vec::new(),
            scroll_speed: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct BgmInfo {
    pub filename: Option<String>,
    #[serde(default = "default_one::<f64>")]
    pub vol: f64,
    #[serde(default = "default_zero::<i32>")]
    pub offset: i32,
    pub preview: PreviewInfo,
    pub legacy: LegacyBgmInfo,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct LegacyBgmInfo {
    pub fp_filenames: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct PreviewInfo {
    #[serde(default = "default_zero::<u32>")]
    pub offset: u32,
    #[serde(default = "default_zero::<u32>")]
    pub duration: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_filename: Option<String>,
}

impl BgmInfo {
    fn new() -> Self {
        BgmInfo {
            filename: None,
            vol: 1.0,
            offset: 0,
            preview: PreviewInfo::default(),
            legacy: LegacyBgmInfo::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KeySoundInfo {
    pub fx: KeySoundFXInfo,
    pub laser: KeySoundLaserInfo,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KeySoundLaserInfo {
    pub vol: ByPulse<f64>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KeySoundFXInfo {
    pub chip_event: HashMap<String, [Vec<ByPulse<KeySoundInvokeFX>>; 2]>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KeySoundInvokeFX {
    pub vol: f64,
}

type NoteParamChange = ByPulseOption<Dict<String>>;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AudioEffectFXInfo {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub def: Dict<AudioEffect>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub param_change: Dict<Dict<ByPulse<String>>>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub long_event: Dict<[Vec<NoteParamChange>; 2]>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AudioEffectLaserInfo {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    def: Dict<AudioEffect>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub param_change: Dict<Dict<ByPulse<String>>>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub pulse_event: Dict<ByPulse<()>>,
    #[serde(default = "default_zero::<i32>")]
    pub peaking_filter_delay: i32,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AudioEffectInfo {
    pub fx: AudioEffectFXInfo,
    pub laser: AudioEffectLaserInfo,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AudioInfo {
    pub bgm: Option<BgmInfo>,
    pub audio_effect: Option<AudioEffectInfo>,
    #[serde(skip_deserializing)]
    pub key_sound: Option<KeySoundInfo>,
}

impl AudioInfo {
    fn new() -> Self {
        AudioInfo {
            key_sound: None,
            audio_effect: None,
            bgm: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Chart {
    pub meta: MetaInfo,
    pub note: NoteInfo,
    pub beat: BeatInfo,
    pub audio: AudioInfo,
    #[serde(default)]
    pub camera: camera::CameraInfo,
    pub version: String,
    pub bg: BgInfo,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BgInfo {
    pub filename: Option<String>,
    #[serde(default)]
    pub offset: i32,
    pub legacy: Option<LegacyBgInfo>,
}

impl BgInfo {
    pub fn new() -> Self {
        Self {
            filename: None,
            offset: 0,
            legacy: None,
        }
    }
}

impl Default for BgInfo {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LegacyBgInfo {
    pub bg: Option<Vec<KshBgInfo>>,
    pub layer: Option<KshLayerInfo>,
    pub movie: Option<KshMovieInfo>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KshLayerInfo {
    pub filename: Option<String>, // self-explanatory (can be KSM default animation layer such as "arrow")
    /// one-loop duration in milliseconds.
    ///
    /// If the value is negative, the animation is played backwards.
    ///
    /// If the value is zero, the play speed is tempo-synchronized and set to 1 frame per 0.035 measure (= 28.571... frames/measure).
    #[serde(default)]
    pub duration: i32,
    pub rotation: Option<KshLayerRotationInfo>, // rotation conditions
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KshLayerRotationInfo {
    pub tilt: bool, // whether lane tilts affect rotation of BG/layer
    pub spin: bool, // whether lane spins affect rotation of BG/layer
}
#[derive(Serialize, Deserialize, Clone)]
pub struct KshMovieInfo {
    pub filename: Option<String>, // self-explanatory
    pub offset: i32,              // movie offset in millisecond
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KshBgInfo {
    pub filename: String,
}

type BeatLineFn = dyn Fn(u32) -> Option<(u32, bool)>;
pub struct MeasureBeatLines {
    tick: u32,
    funcs: Vec<(u32, Box<BeatLineFn>)>,
    func_index: usize,
}

impl Iterator for MeasureBeatLines {
    type Item = (u32, bool);

    fn next(&mut self) -> Option<(u32, bool)> {
        if let Some(func) = self.funcs.get(self.func_index) {
            if let Some((new_tick, is_measure)) = func.1(self.tick) {
                let old_tick = self.tick;
                self.tick = new_tick;
                if let Some(next_func) = self.funcs.get(self.func_index + 1) {
                    if self.tick >= next_func.0 {
                        self.func_index += 1;
                    }
                }

                return Some((old_tick, is_measure));
            }
        }

        None
    }
}

impl Default for Chart {
    fn default() -> Self {
        Self::new()
    }
}

//TODO: Duration based API
impl Chart {
    pub fn new() -> Self {
        Chart {
            meta: MetaInfo::new(),
            note: NoteInfo::new(),
            beat: BeatInfo::new(),
            audio: AudioInfo::new(),
            camera: CameraInfo::default(),
            version: "0.7.0".to_string(),
            bg: BgInfo::new(),
        }
    }

    pub fn mode_bpm(&self) -> Option<f64> {
        let mut last_bpm = *self.beat.bpm.first()?;

        let mut durations: HashMap<u64, f64> = HashMap::new();

        for ab in self.beat.bpm.windows(2) {
            let a = ab[0];
            let b = ab[1];
            let l = durations.entry(a.1.to_bits()).or_default();
            *l += self.tick_to_ms(b.0) - self.tick_to_ms(a.0);

            last_bpm = b;
        }

        {
            let x = durations.entry(last_bpm.1.to_bits()).or_default();
            *x += self.tick_to_ms(self.get_last_tick()) - self.tick_to_ms(last_bpm.0);
        }

        durations
            .iter()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|x| f64::from_bits(*x.0))
    }

    pub fn ms_to_tick(&self, ms: f64) -> u32 {
        if ms <= 0.0 {
            return 0;
        }

        let bpm = match self
            .beat
            .bpm
            .binary_search_by(|b| self.tick_to_ms(b.0).partial_cmp(&ms).unwrap())
        {
            Ok(i) => self.beat.bpm.get(i).unwrap(),
            Err(i) => self.beat.bpm.get(i - 1).unwrap(),
        };

        let remaining = ms - self.tick_to_ms(bpm.0);
        bpm.0 + ticks_from_ms(remaining, bpm.1, KSON_RESOLUTION) as u32
    }

    pub fn tick_to_ms(&self, tick: u32) -> f64 {
        let mut ret: f64 = 0.0;
        let mut prev = self.beat.bpm.first().unwrap_or(&(0, 120.0));

        for b in &self.beat.bpm {
            if b.0 > tick {
                break;
            }
            ret += ms_from_ticks((b.0 - prev.0) as i64, prev.1, KSON_RESOLUTION);
            prev = b;
        }
        ret + ms_from_ticks((tick - prev.0) as i64, prev.1, KSON_RESOLUTION)
    }

    pub fn tick_to_measure(&self, tick: u32) -> u32 {
        let mut ret = 0;
        let mut time_sig_iter = self.beat.time_sig.iter();
        let mut remaining_ticks = tick;
        if let Some(first_sig) = time_sig_iter.next() {
            let mut prev_index = first_sig.0;
            let mut prev_ticks_per_measure = KSON_RESOLUTION * 4 * first_sig.1 .0 / first_sig.1 .1;
            if prev_ticks_per_measure == 0 {
                return ret;
            }
            for current_sig in time_sig_iter {
                let measure_count = current_sig.0 - prev_index;
                let tick_count = measure_count * prev_ticks_per_measure;
                if tick_count > remaining_ticks {
                    break;
                }
                ret += measure_count;
                remaining_ticks -= tick_count;
                prev_index = current_sig.0;
                prev_ticks_per_measure = KSON_RESOLUTION * 4 * current_sig.1 .0 / current_sig.1 .1;
                if prev_ticks_per_measure == 0 {
                    return ret;
                }
            }
            ret += remaining_ticks / prev_ticks_per_measure;
        }
        ret
    }

    pub fn measure_to_tick(&self, measure: u32) -> u32 {
        let mut ret = 0;
        let mut remaining_measures = measure;
        let mut time_sig_iter = self.beat.time_sig.iter();

        if let Some(first_sig) = time_sig_iter.next() {
            let mut prev_index = first_sig.0;
            let mut prev_ticks_per_measure = KSON_RESOLUTION * 4 * first_sig.1 .0 / first_sig.1 .1;
            for current_sig in time_sig_iter {
                let measure_count = current_sig.0 - prev_index;
                if measure_count > remaining_measures {
                    break;
                }
                ret += measure_count * prev_ticks_per_measure;
                remaining_measures -= measure_count;
                prev_index = current_sig.0;
                prev_ticks_per_measure = KSON_RESOLUTION * 4 * current_sig.1 .0 / current_sig.1 .1;
            }
            ret += remaining_measures * prev_ticks_per_measure;
        }
        ret
    }

    pub fn bpm_at_tick(&self, tick: u32) -> f64 {
        match self.beat.bpm.binary_search_by(|b| b.0.cmp(&tick)) {
            Ok(i) => self.beat.bpm.get(i).unwrap().1,
            Err(i) => self.beat.bpm.get(i - 1).unwrap().1,
        }
    }

    pub fn beat_line_iter(&self) -> MeasureBeatLines {
        let mut funcs: Vec<(u32, Box<BeatLineFn>)> = Vec::new();
        let mut prev_start = 0;
        let mut prev_sig = match self.beat.time_sig.first() {
            Some(v) => v,
            None => &(0, TimeSignature(4, 4)),
        };

        for time_sig in &self.beat.time_sig {
            let ticks_per_beat = KSON_RESOLUTION * 4 / time_sig.1 .1;
            let ticks_per_measure = KSON_RESOLUTION * 4 * time_sig.1 .0 / time_sig.1 .1;
            let prev_ticks_per_measure = KSON_RESOLUTION * 4 * prev_sig.1 .0 / prev_sig.1 .1;

            let new_start = prev_start + (time_sig.0 - prev_sig.0) * prev_ticks_per_measure;
            if ticks_per_measure > 0 && ticks_per_beat > 0 {
                funcs.push((
                    new_start,
                    Box::new(move |y| {
                        let adjusted = y - new_start;
                        Some((y + ticks_per_beat, (adjusted % ticks_per_measure) == 0))
                    }),
                ));
            } else {
                funcs.push((new_start, Box::new(|_| None)));
            }

            prev_start = new_start;
            prev_sig = time_sig;
        }

        MeasureBeatLines {
            tick: 0,
            funcs,
            func_index: 0,
        }
    }

    pub fn get_last_tick(&self) -> u32 {
        let mut last_tick = 0;

        //bt
        for i in 0..4 {
            if let Some(last) = &self.note.bt[i].last() {
                last_tick = last_tick.max(last.y + last.l);
            }
        }

        //fx
        for i in 0..2 {
            if let Some(last) = &self.note.fx[i].last() {
                last_tick = last_tick.max(last.y + last.l);
            }
        }

        //laser
        for i in 0..2 {
            for section in &self.note.laser[i] {
                let base_y = section.0;
                if let Some(last) = &section.1.last() {
                    last_tick = last_tick.max(last.ry + base_y);
                }
            }
        }
        last_tick
    }
}

#[cfg(test)]
mod tests {
    use serde_test::Token;

    use crate::parameter::{self, EffectFloat, EffectFreq, EffectParameterValue};

    #[test]
    fn effect_param() {
        let mut param = parameter::EffectParameter {
            on: Some(EffectParameterValue::Freq(
                EffectFreq::Khz(10.0)..=EffectFreq::Khz(20.0),
            )),
            off: EffectParameterValue::Freq(EffectFreq::Hz(500)..=EffectFreq::Hz(500)),
            v: 0.0_f32,
            ..Default::default()
        };

        serde_test::assert_tokens(&param, &[Token::Str("500Hz>10kHz-20kHz")]);

        param.on = None;
        param.off =
            EffectParameterValue::Filename("e9fda14b-d635-4cd8-8c7a-ca12f8d9b78a".to_string());

        serde_test::assert_tokens(
            &param,
            &[Token::Str("e9fda14b-d635-4cd8-8c7a-ca12f8d9b78a")],
        );

        param.off = EffectParameterValue::Sample(100..=100);
        serde_test::assert_tokens(&param, &[Token::Str("100samples")]);
        param.off = EffectParameterValue::Sample(100..=1000);
        serde_test::assert_tokens(&param, &[Token::Str("100samples-1000samples")]);

        param.off = EffectParameterValue::Length(
            EffectFloat::Fraction(1, 2)..=EffectFloat::Fraction(1, 2),
            true,
        );
        serde_test::assert_tokens(&param, &[Token::Str("1/2")]);

        param.off = EffectParameterValue::Switch(false..=false);
        param.on = Some(EffectParameterValue::Switch(false..=true));
        serde_test::assert_tokens(&param, &[Token::Str("off>off-on")]);
    }
}

pub mod camera;
pub mod effects;
mod graph;
mod ksh;
pub mod parameter;
pub mod score_ticks;

use camera::CameraInfo;
use effects::AudioEffect;
pub use graph::*;
pub use ksh::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::str;

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

#[derive(Serialize, Deserialize, Copy, Clone, Default)]
pub struct GraphPoint {
    pub y: u32,
    pub v: f64,
    pub vf: Option<f64>,
    pub a: Option<f64>,
    pub b: Option<f64>,
}

#[derive(Serialize, Deserialize, Copy, Clone)]
pub struct GraphSectionPoint {
    pub ry: u32,
    pub v: f64,
    pub vf: Option<f64>,
    pub a: Option<f64>,
    pub b: Option<f64>,
}

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

#[derive(Serialize, Deserialize, Copy, Clone)]
pub struct Interval {
    pub y: u32,

    #[serde(default = "default_zero")]
    pub l: u32,
}

fn default_zero<T: From<u8>>() -> T {
    T::from(0)
}

fn default_true<T: From<bool>>() -> T {
    T::from(true)
}

// fn default_false<T: From<bool>>() -> T {
//     T::from(false)
// }

#[derive(Serialize, Deserialize, Clone)]
pub struct LaserSection {
    pub y: u32,
    pub v: Vec<GraphSectionPoint>,
    #[serde(default = "default_one")]
    pub wide: u8,
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
    pub title_translit: Option<String>,
    pub subtitle: Option<String>,
    pub artist: String,
    pub artist_translit: Option<String>,
    pub chart_author: String,
    pub difficulty: DifficultyInfo,
    pub level: u8,
    pub disp_bpm: String,
    pub std_bpm: Option<f64>,
    pub jacket_filename: String,
    pub jacket_author: String,
    pub information: Option<String>,
}

impl DifficultyInfo {
    fn new() -> DifficultyInfo {
        DifficultyInfo {
            name: None,
            short_name: None,
            idx: 0,
        }
    }
}

impl MetaInfo {
    fn new() -> MetaInfo {
        MetaInfo {
            title: String::new(),
            title_translit: None,
            subtitle: None,
            artist: String::new(),
            artist_translit: None,
            chart_author: String::new(),
            difficulty: DifficultyInfo::new(),
            level: 1,
            disp_bpm: String::new(),
            std_bpm: None,
            jacket_filename: String::new(),
            jacket_author: String::new(),
            information: None,
        }
    }
}

#[derive(Serialize, Deserialize, Copy, Clone)]
pub struct ByPulse<T> {
    pub y: u32,
    pub v: T,
}

#[derive(Serialize, Deserialize, Copy, Clone)]
pub struct ByBtnNote<T> {
    lane: u64,
    idx: u64,
    v: Option<T>,
    #[serde(default = "default_true")]
    dom: bool,
}

#[derive(Serialize, Deserialize, Copy, Clone)]
pub struct ByLaserNote<T> {
    lane: u64,
    sec: u64,
    idx: u64,
    v: Option<T>,
    #[serde(default = "default_true")]
    dom: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ByNotes<T> {
    bt: Option<Vec<ByBtnNote<T>>>,
    fx: Option<Vec<ByBtnNote<T>>>,
    laser: Option<Vec<ByLaserNote<T>>>,
}

#[derive(Serialize, Deserialize, Copy, Clone)]
pub struct TimeSignature {
    pub n: u32,
    pub d: u32,
}

impl TimeSignature {
    //Parse from "n/d" string
    fn from_str(s: &str) -> Self {
        let mut data = s.split('/');
        let n: u32 = data.next().unwrap_or("4").parse().unwrap_or(4);
        let d: u32 = data.next().unwrap_or("4").parse().unwrap_or(4);

        TimeSignature { n, d }
    }
}

#[derive(Serialize, Deserialize, Copy, Clone)]
pub struct ByMeasureIndex<T> {
    pub idx: u32,
    pub v: T,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BeatInfo {
    pub bpm: Vec<ByPulse<f64>>,
    pub time_sig: Vec<ByMeasureIndex<TimeSignature>>,
    pub resolution: u32,
}

impl BeatInfo {
    fn new() -> Self {
        BeatInfo {
            bpm: Vec::new(),
            time_sig: Vec::new(),
            resolution: 48,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct BgmInfo {
    pub filename: Option<String>,
    #[serde(default = "default_one")]
    pub vol: f64,
    #[serde(default = "default_zero")]
    pub offset: i32,

    pub preview_filename: Option<String>,
    #[serde(default = "default_zero")]
    pub preview_offset: u32,
    #[serde(default = "default_zero")]
    pub preview_duration: u32,
}

impl BgmInfo {
    fn new() -> Self {
        BgmInfo {
            filename: None,
            vol: 1.0,
            offset: 0,

            preview_filename: None,
            preview_offset: 0,
            preview_duration: 15000,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KeySoundInfo;

#[derive(Serialize, Deserialize, Clone)]
pub struct AudioEffectDef {
    #[serde(rename = "type")]
    effect_type: String,
    v: AudioEffect,
    filename: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AudioEffectInfo {
    def: Option<HashMap<String, AudioEffectDef>>,
    pulse_event: Option<HashMap<String, ByPulse<AudioEffect>>>,
    note_event: Option<HashMap<String, ByNotes<AudioEffect>>>,
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

impl Chart {
    pub fn new() -> Self {
        Chart {
            meta: MetaInfo::new(),
            note: NoteInfo::new(),
            beat: BeatInfo::new(),
            audio: AudioInfo::new(),
            camera: CameraInfo::default(),
        }
    }

    pub fn ms_to_tick(&self, ms: f64) -> u32 {
        if ms <= 0.0 {
            return 0;
        }

        let bpm = match self
            .beat
            .bpm
            .binary_search_by(|b| self.tick_to_ms(b.y).partial_cmp(&ms).unwrap())
        {
            Ok(i) => self.beat.bpm.get(i).unwrap(),
            Err(i) => self.beat.bpm.get(i - 1).unwrap(),
        };

        let remaining = ms - self.tick_to_ms(bpm.y);
        bpm.y + ticks_from_ms(remaining, bpm.v, self.beat.resolution) as u32
    }

    pub fn tick_to_ms(&self, tick: u32) -> f64 {
        let mut ret: f64 = 0.0;
        let mut prev = self.beat.bpm.first().unwrap_or(&ByPulse { y: 0, v: 120.0 });

        for b in &self.beat.bpm {
            if b.y > tick {
                break;
            }
            ret += ms_from_ticks((b.y - prev.y) as i64, prev.v, self.beat.resolution);
            prev = b;
        }
        ret + ms_from_ticks((tick - prev.y) as i64, prev.v, self.beat.resolution)
    }

    pub fn tick_to_measure(&self, tick: u32) -> u32 {
        let mut ret = 0;
        let mut time_sig_iter = self.beat.time_sig.iter();
        let mut remaining_ticks = tick;
        if let Some(first_sig) = time_sig_iter.next() {
            let mut prev_index = first_sig.idx;
            let mut prev_ticks_per_measure =
                self.beat.resolution * 4 * first_sig.v.n / first_sig.v.d;
            if prev_ticks_per_measure == 0 {
                return ret;
            }
            for current_sig in time_sig_iter {
                let measure_count = current_sig.idx - prev_index;
                let tick_count = measure_count * prev_ticks_per_measure;
                if tick_count > remaining_ticks {
                    break;
                }
                ret += measure_count;
                remaining_ticks -= tick_count;
                prev_index = current_sig.idx;
                prev_ticks_per_measure =
                    self.beat.resolution * 4 * current_sig.v.n / current_sig.v.d;
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
            let mut prev_index = first_sig.idx;
            let mut prev_ticks_per_measure =
                self.beat.resolution * 4 * first_sig.v.n / first_sig.v.d;
            for current_sig in time_sig_iter {
                let measure_count = current_sig.idx - prev_index;
                if measure_count > remaining_measures {
                    break;
                }
                ret += measure_count * prev_ticks_per_measure;
                remaining_measures -= measure_count;
                prev_index = current_sig.idx;
                prev_ticks_per_measure =
                    self.beat.resolution * 4 * current_sig.v.n / current_sig.v.d;
            }
            ret += remaining_measures * prev_ticks_per_measure;
        }
        ret
    }

    pub fn bpm_at_tick(&self, tick: u32) -> f64 {
        match self.beat.bpm.binary_search_by(|b| b.y.cmp(&tick)) {
            Ok(i) => self.beat.bpm.get(i).unwrap().v,
            Err(i) => self.beat.bpm.get(i - 1).unwrap().v,
        }
    }

    pub fn beat_line_iter(&self) -> MeasureBeatLines {
        let mut funcs: Vec<(u32, Box<BeatLineFn>)> = Vec::new();
        let mut prev_start = 0;
        let mut prev_sig = match self.beat.time_sig.get(0) {
            Some(v) => v,
            None => &ByMeasureIndex {
                idx: 0,
                v: TimeSignature { n: 4, d: 4 },
            },
        };

        for time_sig in &self.beat.time_sig {
            let ticks_per_beat = self.beat.resolution * 4 / time_sig.v.d;
            let ticks_per_measure = self.beat.resolution * 4 * time_sig.v.n / time_sig.v.d;
            let prev_ticks_per_measure = self.beat.resolution * 4 * prev_sig.v.n / prev_sig.v.d;

            let new_start = prev_start + (time_sig.idx - prev_sig.idx) * prev_ticks_per_measure;
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
                let base_y = section.y;
                if let Some(last) = &section.v.last() {
                    last_tick = last_tick.max(last.ry + base_y);
                }
            }
        }
        last_tick
    }
}

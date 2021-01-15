pub mod effects;
pub mod parameter;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::error;
use std::str;

pub trait Graph<T> {
    fn value_at(&self, tick: f64) -> Option<T>;
}

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

impl Graph<f64> for Vec<GraphSectionPoint> {
    fn value_at(&self, tick: f64) -> Option<f64> {
        match self.binary_search_by(|g| g.ry.cmp(&(tick as u32))) {
            Ok(p) /*On a point*/ => Some(self.get(p).unwrap().v),
            Err(p) /*Between points*/ => {
                if p == 0 || p >= self.len() {
                    return None;
                }
                let start_p = self.get(p - 1).unwrap();
                let end_p = self.get(p).unwrap();
                assert!(start_p.ry < end_p.ry);
                let start_v = match start_p.vf {
                    Some(v) => v,
                    None => start_p.v
                };
                let x = (tick - start_p.ry as f64) / (end_p.ry - start_p.ry) as f64;
                let w = end_p.v - start_v;
                let (a,b) = match (start_p.a, start_p.b) {
                    (Some(a), Some(b)) => (a,b),
                    _ => (0.,0.)
                };
                if (a-b).abs() > f64::EPSILON {
                    Some(start_v + do_curve(x, a, b) * w)
                }
                else {
                    Some(start_v + x * w)
                }
            }
        }
    }
}

impl Graph<f64> for LaserSection {
    fn value_at(&self, tick: f64) -> Option<f64> {
        let r_tick = tick - self.y as f64;
        self.v.value_at(r_tick)
    }
}

impl Graph<f64> for Vec<LaserSection> {
    fn value_at(&self, tick: f64) -> Option<f64> {
        match self.binary_search_by(|s| s.y.cmp(&(tick as u32))) {
            Ok(i) => self.get(i).unwrap().value_at(tick),
            Err(i) => {
                if i > 0 {
                    self.get(i - 1).unwrap().value_at(tick)
                } else {
                    None
                }
            }
        }
    }
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
            resolution: 240,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
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
pub struct AudioEffect {
    #[serde(rename = "type")]
    effect_type: String,
    v: Value,
    filename: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AudioEffectInfo {
    def: Option<HashMap<String, AudioEffect>>,
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
}

pub struct MeasureBeatLines {
    tick: u32,
    funcs: Vec<(u32, Box<dyn Fn(u32) -> Option<(u32, bool)>>)>,
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

fn laser_char_to_value(value: u8) -> Result<f64, String> {
    let chars = [
        (b'0'..=b'9').collect::<Vec<u8>>(),
        (b'A'..=b'Z').collect::<Vec<u8>>(),
        (b'a'..=b'o').collect::<Vec<u8>>(),
    ]; //TODO: check for cleaner ways to do this

    let mut i = 0;
    for cr in chars.iter() {
        for c in cr {
            if *c == value {
                return Ok(i as f64 / 50.0);
            }
            i += 1;
        }
    }
    Err(format!("Invalid laser char: '{}'", value as char))
}

impl Chart {
    pub fn new() -> Self {
        Chart {
            meta: MetaInfo::new(),
            note: NoteInfo::new(),
            beat: BeatInfo::new(),
            audio: AudioInfo::new(),
        }
    }

    pub fn from_ksh(data: &str) -> Result<Chart, Box<dyn error::Error>> {
        let mut new_chart = Chart::new();
        let mut num = 4;
        let mut den = 4;
        let data = &data[3..]; //Something about BOM(?)
        let mut parts: Vec<&str> = data.split("\n--").collect();
        let meta = parts.first().unwrap_or(&"").lines();
        let mut bgm = BgmInfo::new();
        for line in meta {
            let line_data: Vec<&str> = line.split('=').collect();
            if line_data.len() < 2 {
                continue;
            }
            let value = String::from(line_data[1]);
            match line_data[0] {
                "title" => new_chart.meta.title = value,
                "artist" => new_chart.meta.artist = value,
                "effect" => new_chart.meta.chart_author = value,
                "jacket" => new_chart.meta.jacket_filename = value,
                "illustrator" => new_chart.meta.jacket_author = value,
                "t" => {
                    if !value.contains('-') {
                        new_chart.beat.bpm.push(ByPulse {
                            y: 0,
                            v: value.parse()?,
                        })
                    }
                }
                "beat" => {}
                "o" => bgm.offset = value.parse()?,
                "m" => bgm.filename = Some(value),
                "level" => {
                    new_chart.meta.level = value.parse::<u8>().unwrap_or(0);
                }
                "difficulty" => {
                    let mut short_name = String::from(&value);
                    short_name.truncate(3);
                    new_chart.meta.difficulty = DifficultyInfo {
                        idx: 0,
                        name: Some(String::from(&value)),
                        short_name: Some(short_name),
                    };
                    new_chart.meta.difficulty.idx = match value.as_ref() {
                        "light" => 0,
                        "challenge" => 1,
                        "extended" => 2,
                        "infinite" => 3,
                        _ => 0,
                    };
                }
                _ => (),
            }
        }
        new_chart.audio.bgm = Some(bgm);
        parts.remove(0);
        let mut y: u32 = 0;
        let mut measure_index = 0;
        let mut last_char: [char; 8] = ['0'; 8];
        last_char[6] = '-';
        last_char[7] = '-';

        let mut long_y: [u32; 8] = [0; 8];
        let mut laser_builder: [LaserSection; 2] = [
            LaserSection {
                y: 0,
                v: Vec::new(),
                wide: 1,
            },
            LaserSection {
                y: 0,
                v: Vec::new(),
                wide: 1,
            },
        ];

        for measure in parts {
            let measure_lines = measure.lines();
            let note_regex = regex::Regex::new("[0-2]{4}\\|")?;
            let line_count = measure.lines().filter(|x| note_regex.is_match(x)).count() as u32;
            if line_count == 0 {
                continue;
            }
            let mut ticks_per_line = (new_chart.beat.resolution * 4 * num / den) / line_count;
            let mut has_read_notes = false;
            for line in measure_lines {
                if note_regex.is_match(line) {
                    //read bt
                    has_read_notes = true;
                    let chars: Vec<char> = line.chars().collect();
                    for i in 0..4 {
                        if chars[i] == '1' {
                            new_chart.note.bt[i].push(Interval { y, l: 0 });
                        } else if chars[i] == '2' && last_char[i] != '2' {
                            long_y[i] = y;
                        } else if chars[i] != '2' && last_char[i] == '2' {
                            let l = y - long_y[i];
                            new_chart.note.bt[i].push(Interval { y: long_y[i], l });
                        }

                        last_char[i] = chars[i];
                    }

                    //read fx
                    for i in 0..2 {
                        if chars[i + 5] == '2' {
                            new_chart.note.fx[i].push(Interval { y, l: 0 })
                        } else if chars[i + 5] == '0'
                            && last_char[i + 4] != '0'
                            && last_char[i + 4] != '2'
                        {
                            new_chart.note.fx[i].push(Interval {
                                y: long_y[i + 4],
                                l: y - long_y[i + 4],
                            })
                        } else if (chars[i + 5] != '0' && chars[i + 5] != '2')
                            && (last_char[i + 4] == '0' || last_char[i + 4] == '2')
                        {
                            long_y[i + 4] = y;
                        }

                        last_char[i + 4] = chars[i + 5];
                    }

                    //read laser
                    for i in 0..2 {
                        if chars[i + 8] == '-' && last_char[i + 6] != '-' {
                            // end laser
                            let v = std::mem::replace(
                                &mut laser_builder[i],
                                LaserSection {
                                    y: 0,
                                    v: Vec::new(),
                                    wide: 1,
                                },
                            );
                            new_chart.note.laser[i].push(v);
                        }
                        if chars[i + 8] != '-' && chars[i + 8] != ':' && last_char[i + 6] == '-' {
                            // new laser
                            laser_builder[i].y = y;
                            laser_builder[i].v.push(GraphSectionPoint::new(
                                0,
                                laser_char_to_value(chars[i + 8] as u8)?,
                            ));
                        } else if chars[i + 8] != ':' && chars[i + 8] != '-' {
                            // new point
                            laser_builder[i].v.push(GraphSectionPoint::new(
                                y - laser_builder[i].y,
                                laser_char_to_value(chars[i + 8] as u8)?,
                            ));
                        }

                        last_char[i + 6] = chars[i + 8];
                    }

                    y += ticks_per_line;
                } else if line.contains('=') {
                    let mut line_data = line.split('=');

                    let line_prop = String::from(line_data.next().unwrap_or(""));
                    let mut line_value = String::from(line_data.next().unwrap_or(""));

                    match line_prop.as_ref() {
                        "beat" => {
                            let new_sig = TimeSignature::from_str(line_value.as_ref());
                            num = new_sig.n;
                            den = new_sig.d;
                            if !has_read_notes {
                                ticks_per_line =
                                    (new_chart.beat.resolution * 4 * num / den) / line_count;
                                new_chart.beat.time_sig.push(ByMeasureIndex {
                                    idx: measure_index,
                                    v: new_sig,
                                });
                            } else {
                                new_chart.beat.time_sig.push(ByMeasureIndex {
                                    idx: measure_index + 1,
                                    v: new_sig,
                                });
                            }
                        }
                        "t" => new_chart.beat.bpm.push(ByPulse {
                            y,
                            v: line_value.parse()?,
                        }),
                        "laserrange_l" => {
                            line_value.truncate(1);
                            laser_builder[0].wide = line_value.parse()?;
                        }
                        "laserrange_r" => {
                            line_value.truncate(1);
                            laser_builder[1].wide = line_value.parse()?;
                        }
                        _ => (),
                    }
                }
            }
            measure_index += 1;
        }
        //set slams
        for i in 0..2 {
            for section in &mut new_chart.note.laser[i] {
                let mut iter = section.v.iter_mut();
                let mut for_removal: HashSet<u32> = HashSet::new();
                let mut prev = iter.next().unwrap();
                while let Some(next) = iter.next() {
                    if (next.ry - prev.ry) <= (new_chart.beat.resolution / 8) {
                        prev.vf = Some(next.v);
                        for_removal.insert(next.ry);
                        if for_removal.contains(&prev.ry) {
                            for_removal.remove(&prev.ry);
                        }
                    }

                    prev = next;
                }
                section.v.retain(|p| !for_removal.contains(&p.ry));
                section.v.retain(|p| {
                    if let Some(vf) = p.vf {
                        vf.ne(&p.v)
                    } else {
                        true
                    }
                });
            }
        }

        Ok(new_chart)
    }

    pub fn ms_to_tick(&self, ms: f64) -> u32 {
        let mut remaining = ms;
        let mut ret: u32 = 0;
        let mut prev = self.beat.bpm.first().unwrap_or(&ByPulse { y: 0, v: 120.0 });

        for b in &self.beat.bpm {
            let new_ms = self.tick_to_ms(b.y);
            if new_ms > ms {
                break;
            }
            ret = b.y;
            remaining = ms - new_ms;
            prev = b;
        }
        ret + ticks_from_ms(remaining, prev.v, self.beat.resolution) as u32
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
        let mut prev = self.beat.bpm.first().unwrap_or(&ByPulse { y: 0, v: 120.0 });

        for b in &self.beat.bpm {
            if b.y > tick {
                break;
            }
            prev = b;
        }
        prev.v
    }

    pub fn beat_line_iter(&self) -> MeasureBeatLines {
        let mut funcs: Vec<(u32, Box<dyn Fn(u32) -> Option<(u32, bool)>>)> = Vec::new();
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

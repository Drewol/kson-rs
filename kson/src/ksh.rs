use std::cmp::Ordering;
use std::io;
use std::io::BufWriter;
use std::io::Write;

use crate::*;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum KshReadError {
    #[error("Laser value out of range: '{0}'")]
    OutOfRangeLaserValue(char),
    #[error("Failed to parse value: '{0}'")]
    ParseError(#[from] std::string::ParseError),
    #[error("Failed to parse value: '{0}'")]
    ParseFloatError(#[from] std::num::ParseFloatError),
    #[error("Failed to parse value: '{0}'")]
    ParseIntError(#[from] std::num::ParseIntError),
    #[error("Encountered an empty laser section")]
    EmptyLaserSection,
}

#[derive(Debug, Error)]
pub enum KshWriteError {
    #[error("Laser value out of range: '{0}'")]
    OutOfRangeLaserValue(f64),
    #[error("IO Error")]
    FileWriteError(#[from] io::Error),
}

pub trait Ksh {
    fn from_ksh(data: &str) -> Result<crate::Chart, KshReadError>;
    fn to_ksh<W>(&self, out: W) -> Result<(), KshWriteError>
    where
        W: std::io::Write;
}

#[inline]
const fn find_laser_char(value: u8) -> u8 {
    if value >= b'0' && value <= b'9' {
        value - b'0'
    } else if value >= b'A' && value <= b'Z' {
        value - b'A' + 10
    } else if value >= b'a' && value <= b'o' {
        value - b'a' + 36
    } else {
        u8::MAX
    }
}

#[inline]
fn laser_char_to_value(value: u8) -> Result<f64, KshReadError> {
    let v = find_laser_char(value);
    if v == u8::MAX {
        Err(KshReadError::OutOfRangeLaserValue(v as char))
    } else {
        Ok(v as f64 / 50.0)
    }
}

fn parse_ksh_zoom_values(data: &str) -> Result<(f64, Option<f64>), KshReadError> {
    let (v, vf): (f64, Option<f64>) = {
        if data.contains(';') {
            let mut values = data.split(';');
            (
                values.next().unwrap_or("0").parse()?,
                values.next().map(|vf| vf.parse::<f64>().unwrap_or(0.)),
            )
        } else {
            (data.parse()?, None)
        }
    };
    let v = v / 100.0;
    let vf = vf.map(|val| val / 100.0);
    Ok((v, vf))
}

#[inline]
const fn is_beat_line(s: &&str) -> bool {
    if s.len() > 9 {
        let chars = s.as_bytes();

        (chars[0] == b'0' || chars[0] == b'1' || chars[0] == b'2')
            && chars[4] == b'|'
            && chars[7] == b'|'
    } else {
        false
    }
}

const LASER_CHARS: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmno";

#[inline]
fn laser_value_to_char(v: f64) -> Result<char, KshWriteError> {
    if !(0.0..=1.0).contains(&v) {
        Err(KshWriteError::OutOfRangeLaserValue(v))
    } else {
        let i = (v * (LASER_CHARS.len() - 1) as f64).round() as usize;
        Ok(LASER_CHARS.chars().nth(i).unwrap())
    }
}

impl Ksh for crate::Chart {
    fn from_ksh(data: &str) -> Result<crate::Chart, KshReadError> {
        let mut new_chart = Chart::new();
        let mut num = 4;
        let mut den = 4;
        //BOM check
        let data = if data.starts_with(&String::from_utf8_lossy(&[0xEF, 0xBB, 0xBF]).to_string()) {
            &data[3..]
        } else {
            data
        };
        let mut parts: Vec<&str> = data.split("\n--").collect();
        let meta = parts.first().unwrap_or(&"").lines();
        let mut bgm = BgmInfo::new();

        //TODO
        new_chart.beat.scroll_speed = vec![GraphPoint {
            y: 0,
            v: 1.0,
            ..Default::default()
        }];

        let mut legacy_bg: Option<LegacyBgInfo> = None;

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
                    if let Ok(v) = value.parse::<f64>() {
                        new_chart.beat.bpm.push((0, v))
                    }
                    new_chart.meta.disp_bpm = value.clone();
                }
                "beat" => {}
                "o" => bgm.offset = value.parse()?,
                "m" => {
                    let mut filenames = value.split(';').map(String::from);
                    bgm.filename = filenames.next();
                    bgm.legacy.fp_filenames = filenames.collect();
                }
                "level" => {
                    new_chart.meta.level = value.parse::<u8>().unwrap_or(0);
                }
                "difficulty" => {
                    let mut short_name = String::from(&value);
                    short_name.truncate(3);
                    new_chart.meta.difficulty = match value.as_ref() {
                        "light" => 0,
                        "challenge" => 1,
                        "extended" => 2,
                        "infinite" => 3,
                        _ => 0,
                    };
                }
                "plength" => bgm.preview.duration = value.parse()?,
                "po" => bgm.preview.offset = value.parse()?,
                "mvol" => bgm.vol = value.parse::<f64>()? / 100.0,
                "layer" => {
                    //TODO: parse properly
                    legacy_bg = Some(LegacyBgInfo {
                        bg: None,
                        layer: Some(KshLayerInfo {
                            filename: Some(value),
                            duration: 0,
                            rotation: None,
                        }),
                        movie: None,
                    })
                }
                _ => (),
            }
        }

        new_chart.bg.legacy = legacy_bg;
        new_chart.audio.bgm = Some(bgm);
        parts.remove(0);
        let mut y: u32 = 0;
        let mut measure_index = 0;
        let mut last_char: [u8; 8] = [b'0'; 8];
        last_char[6] = b'-';
        last_char[7] = b'-';

        let mut long_y: [u32; 8] = [0; 8];
        let mut laser_builder: [LaserSection; 2] = [
            LaserSection(0, Vec::new(), 1),
            LaserSection(0, Vec::new(), 1),
        ];

        for measure in parts {
            let measure_lines = measure.lines();
            let line_count = measure.lines().filter(is_beat_line).count() as u32;
            if line_count == 0 {
                continue;
            }
            let mut ticks_per_line = (new_chart.beat.resolution * 4 * num / den) / line_count;
            let mut has_read_notes = false;
            for line in measure_lines {
                if is_beat_line(&line) {
                    //read bt
                    has_read_notes = true;
                    let chars = line.as_bytes();
                    for i in 0..4 {
                        if chars[i] == b'1' {
                            new_chart.note.bt[i].push(Interval { y, l: 0 });
                        } else if chars[i] == b'2' && last_char[i] != b'2' {
                            long_y[i] = y;
                        } else if chars[i] != b'2' && last_char[i] == b'2' {
                            let l = y - long_y[i];
                            new_chart.note.bt[i].push(Interval { y: long_y[i], l });
                        }

                        last_char[i] = chars[i];
                    }

                    //read fx
                    for i in 0..2 {
                        if chars[i + 5] == b'2' {
                            new_chart.note.fx[i].push(Interval { y, l: 0 })
                        } else if chars[i + 5] == b'0'
                            && last_char[i + 4] != b'0'
                            && last_char[i + 4] != b'2'
                        {
                            new_chart.note.fx[i].push(Interval {
                                y: long_y[i + 4],
                                l: y - long_y[i + 4],
                            })
                        } else if (chars[i + 5] != b'0' && chars[i + 5] != b'2')
                            && (last_char[i + 4] == b'0' || last_char[i + 4] == b'2')
                        {
                            long_y[i + 4] = y;
                        }

                        last_char[i + 4] = chars[i + 5];
                    }

                    //read laser
                    for i in 0..2 {
                        if chars[i + 8] == b'-' && last_char[i + 6] != b'-' {
                            // end laser
                            let v = std::mem::replace(
                                &mut laser_builder[i],
                                LaserSection(0, Vec::new(), 1),
                            );
                            new_chart.note.laser[i].push(v);
                        }
                        if chars[i + 8] != b'-' && chars[i + 8] != b':' && last_char[i + 6] == b'-'
                        {
                            // new laser
                            laser_builder[i].0 = y;
                            laser_builder[i].1.push(GraphSectionPoint::new(
                                0,
                                laser_char_to_value(chars[i + 8])?,
                            ));
                        } else if chars[i + 8] != b':' && chars[i + 8] != b'-' {
                            // new point
                            laser_builder[i].1.push(GraphSectionPoint::new(
                                y - laser_builder[i].0,
                                laser_char_to_value(chars[i + 8])?,
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
                            let sig_idx = if has_read_notes {
                                measure_index + 1
                            } else {
                                measure_index
                            };

                            num = new_sig.0;
                            den = new_sig.1;
                            if !has_read_notes {
                                ticks_per_line =
                                    (new_chart.beat.resolution * 4 * num / den) / line_count;
                            }
                            new_chart.beat.time_sig.push((sig_idx, new_sig));
                        }
                        "t" => new_chart.beat.bpm.push((y, line_value.parse()?)),
                        "laserrange_l" => {
                            line_value.truncate(1);
                            laser_builder[0].2 = line_value.parse()?;
                        }
                        "laserrange_r" => {
                            line_value.truncate(1);
                            laser_builder[1].2 = line_value.parse()?;
                        }
                        "zoom_bottom" => {
                            let (v, vf) = parse_ksh_zoom_values(&line_value)?;
                            new_chart.camera.cam.body.zoom.push(GraphPoint {
                                y,
                                v,
                                vf,
                                ..Default::default()
                            })
                        }
                        "zoom_top" => {
                            let (v, vf) = parse_ksh_zoom_values(&line_value)?;
                            new_chart.camera.cam.body.rotation_x.push(GraphPoint {
                                y,
                                v,
                                vf,
                                ..Default::default()
                            })
                        }
                        "zoom_side" => {
                            let (v, vf) = parse_ksh_zoom_values(&line_value)?;
                            new_chart.camera.cam.body.shift_x.push(GraphPoint {
                                y,
                                v,
                                vf,
                                ..Default::default()
                            })
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
                if section.1.is_empty() {
                    return Err(KshReadError::EmptyLaserSection);
                }
                let mut iter = section.1.iter_mut();
                let mut for_removal: HashSet<u32> = HashSet::new();
                let mut prev = iter.next().unwrap();
                for next in iter {
                    if (next.ry - prev.ry) <= (new_chart.beat.resolution / 8) {
                        prev.vf = Some(next.v);
                        for_removal.insert(next.ry);
                        if for_removal.contains(&prev.ry) {
                            for_removal.remove(&prev.ry);
                        }
                    }

                    prev = next;
                }
                section.1.retain(|p| !for_removal.contains(&p.ry));
                section.1.retain(|p| {
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

    //TODO: Write optimized charts using lcm, also ksm doesn't seem to like resolution > 48
    fn to_ksh<W>(&self, out: W) -> Result<(), KshWriteError>
    where
        W: std::io::Write,
    {
        let mut w = BufWriter::new(out);

        //Meta
        {
            writeln!(&mut w, "title={}\r", self.meta.title)?;
            writeln!(&mut w, "artist={}\r", self.meta.artist)?;
            writeln!(&mut w, "effect={}\r", self.meta.chart_author)?;

            let diff = match self.meta.difficulty {
                0 => "light",
                1 => "challenge",
                2 => "extended",
                _ => "infinite",
            };

            writeln!(&mut w, "difficulty={}\r", diff)?;
            writeln!(&mut w, "level={}\r", self.meta.level)?;
            writeln!(&mut w, "jacket={}\r", self.meta.jacket_filename)?;
            writeln!(&mut w, "illustrator={}\r", self.meta.jacket_author)?;
            let bgm = self.audio.bgm.clone().unwrap_or_default();
            writeln!(&mut w, "m={}\r", bgm.filename.unwrap_or_default())?;
            writeln!(&mut w, "o={}\r", bgm.offset)?;
            writeln!(&mut w, "po={}\r", bgm.preview.offset)?;
            if self.beat.bpm.len() == 1 {
                writeln!(&mut w, "t={}\r", self.beat.bpm.first().unwrap().1)?;
            } else {
                let bpm_cmp = |a: &&(u32, f64), b: &&(u32, f64)| match a.1.partial_cmp(&b.1) {
                    Some(ord) => ord,
                    None => Ordering::Equal,
                };

                writeln!(
                    &mut w,
                    "t={:.1}-{:.1}\r",
                    self.beat.bpm.iter().min_by(bpm_cmp).unwrap().1,
                    self.beat.bpm.iter().max_by(bpm_cmp).unwrap().1
                )?;
            }
            writeln!(&mut w, "plength={}\r", bgm.preview.duration)?;
            writeln!(
                &mut w,
                "information={}\r",
                self.meta.information.clone().unwrap_or_default()
            )?;
            writeln!(&mut w, "ver=171\r")?;
            writeln!(&mut w, "--\r")?;
        }

        let mut measure = 0;
        let mut last_laser_write_y = [u32::MAX, u32::MAX];
        let mut last_laser_write_v = [char::MAX, char::MAX];
        let last_tick = self.get_last_tick();
        let mut slam_pending = [None; 2];
        loop {
            let measure_tick = self.measure_to_tick(measure);
            if measure_tick > last_tick {
                break;
            }

            if let Ok(i) = self.beat.time_sig.binary_search_by(|f| f.0.cmp(&measure)) {
                let sig = self.beat.time_sig.get(i).unwrap();

                writeln!(&mut w, "beat={}/{}\r", sig.1 .0, sig.1 .1)?;
            }

            let next_measure_tick = self.measure_to_tick(measure + 1);
            let slam_distance = self.beat.resolution / 8;
            for y in measure_tick..next_measure_tick {
                //Tick events
                {
                    //BPM
                    if let Ok(b) = self.beat.bpm.binary_search_by(|f| f.0.cmp(&y)) {
                        if (y > 0 && self.beat.bpm.len() == 1) || self.beat.bpm.len() > 1 {
                            let bpm = self.beat.bpm.get(b).unwrap();
                            writeln!(&mut w, "t={}\r", bpm.1)?;
                        }
                    }

                    //Laser width
                    if let Ok(b) = self.note.laser[0].binary_search_by(|f| f.0.cmp(&y)) {
                        let l = self.note.laser[0].get(b).unwrap();
                        if l.2 == 2 {
                            writeln!(&mut w, "laserrange_l=2x\r")?;
                        }
                    }
                    if let Ok(b) = self.note.laser[1].binary_search_by(|f| f.0.cmp(&y)) {
                        let l = self.note.laser[1].get(b).unwrap();
                        if l.2 == 2 {
                            writeln!(&mut w, "laserrange_r=2x\r")?;
                        }
                    }

                    //Camera Pos
                }

                //BT
                for l in &self.note.bt {
                    match l.binary_search_by(|f| f.y.cmp(&y)) {
                        Ok(i) => {
                            let note = l.get(i).unwrap();
                            if note.l == 0 {
                                w.write_all(b"1")?;
                            } else {
                                w.write_all(b"2")?;
                            }
                        }
                        Err(i) => {
                            if i == 0 {
                                w.write_all(b"0")?;
                                continue;
                            }
                            if let Some(note) = l.get(i - 1) {
                                if y < note.y + note.l {
                                    w.write_all(b"2")?;
                                } else {
                                    w.write_all(b"0")?;
                                }
                            } else {
                                w.write_all(b"0")?;
                            }
                        }
                    }
                }
                w.write_all(b"|")?;

                //FX
                for l in &self.note.fx {
                    match l.binary_search_by(|f| f.y.cmp(&y)) {
                        Ok(i) => {
                            let note = l.get(i).unwrap();
                            if note.l == 0 {
                                w.write_all(b"2")?;
                            } else {
                                w.write_all(b"1")?;
                            }
                        }
                        Err(i) => {
                            if i == 0 {
                                w.write_all(b"0")?;
                                continue;
                            }
                            if let Some(note) = l.get(i - 1) {
                                if y < note.y + note.l {
                                    w.write_all(b"1")?;
                                } else {
                                    w.write_all(b"0")?;
                                }
                            } else {
                                w.write_all(b"0")?;
                            }
                        }
                    }
                }
                w.write_all(b"|")?;

                //Lasers
                //TODO: Clean up
                for (li, l) in self.note.laser.iter().enumerate() {
                    match l.binary_search_by(|f| f.0.cmp(&y)) {
                        Ok(i) => {
                            let section = l.get(i).unwrap();
                            if let Some(s) = section.1.first() {
                                let ksh_v = laser_value_to_char(s.v)?;
                                w.write_all(&[ksh_v as u8])?;
                                last_laser_write_y[li] = y;
                                slam_pending[li] = s.vf;
                            }
                        }
                        Err(i) => {
                            if i == 0 {
                                w.write_all(b"-")?;
                                continue;
                            }
                            if let Some(s) = l.get(i - 1) {
                                let ry = y - s.0;
                                match s.1.binary_search_by(|f| f.ry.cmp(&ry)) {
                                    Ok(point_i) => {
                                        let point = s.1.get(point_i).unwrap();
                                        let ksh_v = laser_value_to_char(point.v)?;
                                        w.write_all(&[ksh_v as u8])?;
                                        last_laser_write_v[li] = ksh_v;
                                        last_laser_write_y[li] = y;
                                        slam_pending[li] = point.vf;
                                    }
                                    Err(point_i) => {
                                        if point_i == 0 {
                                            //before laser
                                            if let Some(v) = slam_pending[li] {
                                                if y == last_laser_write_y[li] + slam_distance {
                                                    let ksh_v = laser_value_to_char(v)?;
                                                    w.write_all(&[ksh_v as u8])?;
                                                    last_laser_write_v[li] = ksh_v;
                                                    last_laser_write_y[li] = y;
                                                    slam_pending[li] = None;
                                                } else {
                                                    w.write_all(b":")?;
                                                }
                                            }
                                        } else if point_i < s.1.len() {
                                            //on laser
                                            let point = s.1.get(point_i - 1).unwrap();
                                            // Slam
                                            if let Some(v) = point.vf {
                                                if last_laser_write_y[li] == s.0 + point.ry
                                                    && y == last_laser_write_y[li] + slam_distance
                                                {
                                                    let ksh_v = laser_value_to_char(v)?;
                                                    w.write_all(&[ksh_v as u8])?;
                                                    last_laser_write_v[li] = ksh_v;
                                                    last_laser_write_y[li] = y;
                                                    slam_pending[li] = None;
                                                } else {
                                                    w.write_all(b":")?;
                                                }
                                            } else {
                                                //interpolate curve
                                                match (point.a, point.b) {
                                                    (Some(a), Some(b))
                                                        if (a - b).abs() > f64::EPSILON =>
                                                    {
                                                        let delta = (y - last_laser_write_y[li])
                                                            .min(
                                                                s.1.get(point_i)
                                                                    .map(|e| e.ry - ry)
                                                                    .unwrap_or(u32::MAX),
                                                            );
                                                        if delta > slam_distance * 2
                                                            && (a - b).abs() > f64::EPSILON
                                                        {
                                                            let ksh_v = laser_value_to_char(
                                                                s.value_at(y as f64).unwrap(),
                                                            )?;
                                                            if ksh_v != last_laser_write_v[li] {
                                                                w.write_all(&[ksh_v as u8])?;
                                                                last_laser_write_y[li] = y;
                                                                last_laser_write_v[li] = ksh_v;
                                                            } else {
                                                                w.write_all(b":")?;
                                                            }
                                                        } else {
                                                            w.write_all(b":")?;
                                                        }
                                                    }
                                                    _ => w.write_all(b":")?,
                                                }
                                            }
                                        } else {
                                            //after laser
                                            let point = s.1.get(point_i - 1).unwrap();
                                            if let Some(v) = point.vf {
                                                if last_laser_write_y[li] == s.0 + point.ry
                                                    && y == last_laser_write_y[li] + slam_distance
                                                {
                                                    let ksh_v = laser_value_to_char(v)?;
                                                    w.write_all(&[ksh_v as u8])?;
                                                    last_laser_write_v[li] = ksh_v;
                                                    last_laser_write_y[li] = y;
                                                    slam_pending[li] = None;
                                                } else if last_laser_write_y[li] == s.0 + point.ry
                                                    && y < last_laser_write_y[li] + slam_distance
                                                {
                                                    w.write_all(b":")?;
                                                } else {
                                                    w.write_all(b"-")?;
                                                }
                                            } else {
                                                w.write_all(b"-")?;
                                            }
                                        }
                                    }
                                }
                            } else {
                                w.write_all(b"-")?;
                            }
                        }
                    }
                }
                w.write_all(b"\r\n")?;
            }

            writeln!(&mut w, "--\r")?;
            measure += 1;
        }

        Ok(())
    }
}

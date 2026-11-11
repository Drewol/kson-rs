use std::io;
use std::io::BufWriter;
use std::io::Write;
use std::ops::Div;

use crate::camera::NamedTiltValue;
use crate::camera::TiltValue;
use crate::*;

use convert_case::Case;
use convert_case::Casing;
use effects::Effect;
use thiserror::Error;

use self::camera::CamPatternInvokeSpin;
use self::camera::CamPatternInvokeSwing;
use self::camera::CamPatternInvokeSwingValue;

#[derive(Debug, Error)]
pub enum KshReadErrorDetails {
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
    #[error("Invalid tilt value: '{0}'")]
    InvalidTiltValue(String),
    #[error("Invalid curve value: '{0}'")]
    InvalidCurveValue(String),
    #[error("Invalid parameter key: '{0}'")]
    InvalidParameterKey(String),
}

#[derive(Debug, Error)]
pub struct KshReadError {
    error: KshReadErrorDetails,
    line: usize,
}

impl std::fmt::Display for KshReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line == usize::MAX {
            self.error.fmt(f)
        } else {
            f.write_fmt(format_args!("Error on line {}: {}", self.line, self.error))
        }
    }
}

impl KshReadErrorDetails {
    fn at_line(self, line: usize) -> KshReadError {
        KshReadError { error: self, line }
    }
}

trait WithLine<T> {
    fn with_line(self, line: usize) -> Result<T, KshReadError>;
}

impl<T, E> WithLine<T> for Result<T, E>
where
    E: Into<KshReadErrorDetails>,
{
    fn with_line(self, line: usize) -> Result<T, KshReadError> {
        self.map_err(|x| x.into().at_line(line))
    }
}

#[derive(Debug, Error)]
pub enum KshWriteError {
    #[error("Laser value out of range: '{0}'")]
    OutOfRangeLaserValue(f64),
    #[error("IO Error")]
    FileWriteError(#[from] io::Error),
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

const fn legacy_effect_map(value: u8) -> &'static str {
    match value {
        b'S' => "Retrigger;8",
        b'V' => "Retrigger;12",
        b'T' => "Retrigger;16",
        b'W' => "Retrigger;24",
        b'U' => "Retrigger;32",
        b'G' => "Gate;4",
        b'H' => "Gate;8",
        b'K' => "Gate;12",
        b'I' => "Gate;16",
        b'L' => "Gate;24",
        b'J' => "Gate;32",
        b'F' => "Flanger",
        b'P' => "PitchShift;12",
        b'B' => "BitCrusher;5",
        b'Q' => "Phaser",
        b'X' => "Wobble;12",
        b'A' => "TapeStop",
        b'D' => "SideChain",
        _ => "",
    }
}

#[inline]
fn laser_char_to_value(value: u8, wide: u8) -> Result<f64, KshReadErrorDetails> {
    match (wide > 1, value) {
        (true, b'C') => Ok(0.25), // Center wide laser on left laser origin
        (true, b'b') => Ok(0.75), // Center wide laser on right laser origin
        _ => {
            let v = find_laser_char(value);
            if v == u8::MAX {
                Err(KshReadErrorDetails::OutOfRangeLaserValue(value as char))
            } else {
                Ok(v as f64 / 50.0)
            }
        }
    }
}

fn parse_ksh_zoom_values(data: &str) -> Result<(f64, Option<f64>), KshReadErrorDetails> {
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
    Ok((v, vf))
}

#[inline]
fn parse_ksh_curve_value(data: &str) -> Result<(f64, f64), KshReadErrorDetails> {
    let (a, b) = data
        .split_once(';')
        .ok_or_else(|| KshReadErrorDetails::InvalidCurveValue(data.to_string()))?;

    let a = a.parse::<f64>()?;
    let b = b.parse::<f64>()?;

    Ok((a.clamp(0.0, 1.0), b.clamp(0.0, 1.0)))
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
    let i = (v * (LASER_CHARS.len() - 1) as f64).round() as usize;
    LASER_CHARS
        .chars()
        .nth(i)
        .ok_or(KshWriteError::OutOfRangeLaserValue(v))
}

fn split_fx_string(v: String) -> (String, Option<String>, Option<String>) {
    let mut v = v.split(';');
    (
        v.next().unwrap_or_default().to_string(),
        v.next().map(|x| x.to_string()),
        v.next().map(|x| x.to_string()),
    )
}

/// Maps a KSH effect name to the kson effect name (user-defined names are kept as-is)
fn ksh_effect_name(name: &str) -> String {
    AudioEffect::ksh_name_to_kson(name)
        .map(str::to_string)
        .unwrap_or_else(|| name.to_string())
}

/// Maps a KSH (camelCase) effect parameter name to the kson parameter name
fn ksh_param_name(name: &str) -> String {
    match name {
        "fileName" => "filename".to_string(),
        "loFreq" => "freq_1".to_string(),
        "hiFreq" => "freq_2".to_string(),
        v => v.to_case(Case::Snake),
    }
}

/// Parses the lane spin / swing part of a chart line (e.g. `@(192`, `S<192;250;3;2`)
fn parse_lane_spin(chart: &mut Chart, y: u32, chars: &[u8]) {
    let params = String::from_utf8_lossy(&chars[12..]);
    let mut params = params.split(';');
    let Some(Ok(length)) = params.next().map(|x| x.trim().parse::<u32>()) else {
        return;
    };
    let length = (length * 4 * KSON_RESOLUTION) / 192;
    let slam_event = &mut chart.camera.cam.pattern.laser.slam_event;

    match (chars[10], chars[11]) {
        (b'@', b'<') => slam_event
            .half_spin
            .push(CamPatternInvokeSpin(y, -1, length)),
        (b'@', b'>') => slam_event.half_spin.push(CamPatternInvokeSpin(y, 1, length)),
        (b'@', b'(') => slam_event.spin.push(CamPatternInvokeSpin(y, -1, length)),
        (b'@', b')') => slam_event.spin.push(CamPatternInvokeSpin(y, 1, length)),
        (b'S', d @ (b'<' | b'>')) => {
            let default = CamPatternInvokeSwingValue::default();
            let mut next = || params.next().and_then(|x| x.trim().parse::<i64>().ok());
            let value = CamPatternInvokeSwingValue {
                scale: next().map(|x| x as f32).unwrap_or(default.scale),
                repeat: next().map(|x| x.max(0) as u32).unwrap_or(default.repeat),
                decay_order: next()
                    .map(|x| x.clamp(0, 2) as u32)
                    .unwrap_or(default.decay_order),
            };
            slam_event.swing.push(CamPatternInvokeSwing(
                y,
                if d == b'<' { -1 } else { 1 },
                length,
                value,
            ))
        }
        _ => {}
    }
}

const PLACEHOLDER_PARAM_1: &str = "_p1";
const PLACEHOLDER_PARAM_2: &str = "_p2";

impl crate::Chart {
    pub fn from_ksh(data: &str) -> Result<crate::Chart, KshReadError> {
        let mut new_chart = Chart::new();
        let mut num = 4;
        let mut den = 4;
        //BOM check
        let data = data.strip_prefix('\u{feff}').unwrap_or(data);
        let mut parts: Vec<&str> = data.split("\n--").collect();
        let meta = parts.first().unwrap_or(&"").lines();
        let mut bgm = BgmInfo::new();
        bgm.preview.duration = 0;
        new_chart.audio.audio_effect.laser.peaking_filter_delay = 40;
        new_chart.beat.scroll_speed = vec![];

        let mut legacy_bg: Option<LegacyBgInfo> = None;
        let mut unknown = KshUnknownInfo::default();
        let mut comments: ByPulse<String> = vec![];
        let mut ver: Option<String> = None;
        let mut ver_compat: Option<String> = None;
        let mut header_filtertype: Option<String> = None;
        let mut header_vol: Option<f64> = None;
        let mut header_filter_gain: Option<f64> = None;
        let mut mvol: Option<f64> = None;
        let mut layer_value: Option<String> = None;
        let mut movie = KshMovieInfo {
            filename: None,
            offset: 0,
        };
        let mut file_line = 0;
        for (line_idx, line) in meta.enumerate() {
            file_line = line_idx + 1;
            let line = line.trim_end_matches('\r');
            if let Some(comment) = line.strip_prefix("//") {
                comments.push((0, comment.to_string()));
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                if !line.trim().is_empty() {
                    unknown.line.push((0, line.to_string()));
                }
                continue;
            };
            let value = value.to_string();

            match key {
                "title" => new_chart.meta.title = value,
                "title_translit" => new_chart.meta.title_translit = Some(value),
                "title_img" => new_chart.meta.title_img_filename = Some(value),
                "artist" => new_chart.meta.artist = value,
                "artist_translit" => new_chart.meta.artist_translit = Some(value),
                "artist_img" => new_chart.meta.artist_img_filename = Some(value),
                "effect" => new_chart.meta.chart_author = value,
                "jacket" => new_chart.meta.jacket_filename = value,
                "illustrator" => new_chart.meta.jacket_author = value,
                "chokkakuvol" => {
                    header_vol = Some(
                        value
                            .parse::<f64>()
                            .with_line(file_line)?
                            .div(100.0)
                            .clamp(0.0, 1.0),
                    )
                }
                "chokkakuautovol" => {
                    new_chart
                        .audio
                        .key_sound
                        .laser
                        .legacy
                        .get_or_insert_default()
                        .vol_auto = value.trim() == "1"
                }
                "t" => {
                    if let Ok(v) = value.parse::<f64>() {
                        new_chart.beat.bpm.push((0, v))
                    }
                    new_chart.meta.disp_bpm.clone_from(&value);
                }
                "to" => {
                    if let Ok(v) = value.parse::<f64>() {
                        if v > 0.0 {
                            new_chart.meta.std_bpm = Some(v);
                        }
                    }
                }
                "beat" => {
                    let sig = TimeSignature::from_str(&value);
                    num = sig.0;
                    den = sig.1;
                    new_chart.beat.time_sig = vec![(0, sig)];
                }
                "total" => {
                    let total = value.parse::<u32>().with_line(file_line)?;
                    if total > 0 {
                        new_chart.gauge = Some(GaugeInfo { total });
                    }
                }
                "o" => bgm.offset = value.parse::<i32>().with_line(file_line)?,
                "m" => {
                    let mut filenames = value.split(';').map(String::from);
                    bgm.filename = filenames.next().unwrap_or_default();
                    bgm.legacy.fp_filenames = filenames.collect();
                }
                "level" => {
                    new_chart.meta.level = value.parse::<u8>().unwrap_or(1);
                }
                "difficulty" => {
                    new_chart.meta.difficulty = match value.as_ref() {
                        "light" => 0,
                        "challenge" => 1,
                        "extended" => 2,
                        _ => 3, // "infinite" and unknown difficulties
                    };
                }
                "plength" => bgm.preview.duration = value.parse().with_line(file_line)?,
                "po" => bgm.preview.offset = value.parse().with_line(file_line)?,
                "mvol" => mvol = Some(value.parse::<f64>().with_line(file_line)? / 100.0),
                "ver" => ver = Some(value),
                "ver_compat" => ver_compat = Some(value),
                "information" => new_chart.meta.information = Some(value),
                "filtertype" => header_filtertype = Some(value),
                "pfiltergain" => {
                    header_filter_gain =
                        Some(value.parse::<f64>().with_line(file_line)?.div(100.0))
                }
                "pfilterdelay" => {
                    new_chart.audio.audio_effect.laser.peaking_filter_delay =
                        value.parse().with_line(file_line)?
                }
                "v" => movie.filename = Some(value),
                "vo" => movie.offset = value.parse().with_line(file_line)?,
                "layer" => layer_value = Some(value),
                "bg" => {
                    let bg = legacy_bg.get_or_insert_default();
                    bg.bg = Some(
                        value
                            .split(';')
                            .take(2)
                            .map(|filename| KshBgInfo {
                                filename: filename.to_string(),
                            })
                            .collect(),
                    );
                }
                _ => {
                    unknown.meta.insert(key.to_string(), value);
                }
            }
        }

        // Version dependent behavior ("ver_compat" takes priority over "ver")
        let behavior_ver = ksh_version_number(
            ver_compat
                .as_deref()
                .or(ver.as_deref())
                .unwrap_or("100"),
        );
        let syntax_ver = ksh_version_number(ver.as_deref().unwrap_or("100"));
        if behavior_ver < 200 {
            new_chart.compat.get_or_insert_default().ksh_version =
                ver.clone().unwrap_or_else(|| "100".to_string());
        }

        bgm.vol = mvol.unwrap_or(1.0) * if ver.is_none() { 0.6 } else { 1.0 };

        if let Some(layer) = layer_value {
            let separator = if syntax_ver < 166 { '/' } else { ';' };
            let mut layer_parts = layer.split(separator);
            let filename = layer_parts.next().unwrap_or_default().to_string();
            let duration = layer_parts
                .next()
                .and_then(|x| x.parse().ok())
                .unwrap_or(0);
            let rotation = layer_parts
                .next()
                .and_then(|x| x.parse::<u8>().ok())
                .map(|r| KshLayerRotationInfo {
                    tilt: r & 1 != 0,
                    spin: r & 2 != 0,
                });
            legacy_bg.get_or_insert_default().layer = Some(KshLayerInfo {
                filename: Some(filename),
                duration,
                rotation,
            });
        }
        if movie.filename.is_some() {
            legacy_bg.get_or_insert_default().movie = Some(movie);
        }

        new_chart.audio.key_sound.laser.vol = vec![(0, header_vol.unwrap_or(0.5))];
        if let Some(gain) = header_filter_gain {
            new_chart
                .audio
                .audio_effect
                .laser
                .legacy
                .get_or_insert_default()
                .filter_gain = vec![(0, gain)];
        }

        let filtertype = header_filtertype.unwrap_or_else(|| "peak".to_string());
        new_chart
            .audio
            .audio_effect
            .laser
            .pulse_event
            .entry(ksh_effect_name(&filtertype))
            .or_default()
            .push(0);

        new_chart.bg.legacy = legacy_bg;
        new_chart.audio.bgm = bgm;
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

        let mut laser_curves = [(0.5, 0.5); 2];

        let mut fx_string: [Option<String>; 2] = [None, None];
        let mut fx_legacy_param: [Option<String>; 2] = [None, None];
        let mut graph_curves: HashMap<&'static str, Vec<(u32, (f64, f64))>> = HashMap::new();
        let zoom_limit = if syntax_ver < 167 { 300.0 } else { 65535.0 };

        for measure in parts {
            let measure_lines = measure.lines();
            let line_count = measure.lines().filter(is_beat_line).count() as u32;
            let mut ticks_per_line = (KSON_RESOLUTION * 4 * num / den) / line_count.max(1);
            let mut has_read_notes = false;
            for line in measure_lines {
                let line = line.trim_end_matches('\r');
                file_line += 1;
                if is_beat_line(&line) {
                    //read bt
                    has_read_notes = true;
                    let chars = line.as_bytes();
                    for i in 0..4 {
                        // end of a long note
                        if chars[i] != b'2' && last_char[i] == b'2' {
                            let l = y - long_y[i];
                            new_chart.note.bt[i].push(Interval { y: long_y[i], l });
                        }

                        if chars[i] == b'1' {
                            new_chart.note.bt[i].push(Interval { y, l: 0 });
                        } else if chars[i] == b'2' && last_char[i] != b'2' {
                            long_y[i] = y;
                        }

                        last_char[i] = chars[i];
                    }

                    //read fx
                    for i in 0..2 {
                        let c = chars[i + 5];
                        let last = last_char[i + 4];
                        let is_long = |c: u8| c != b'0' && c != b'2';

                        // end of a long note
                        if !is_long(c) && is_long(last) {
                            new_chart.note.fx[i].push(Interval {
                                y: long_y[i + 4],
                                l: y - long_y[i + 4],
                            });

                            // Legacy effect characters take priority over "fx-l"/"fx-r"
                            let legacy_string = legacy_effect_map(last);
                            let effect_string = if !legacy_string.is_empty() {
                                fx_string[i] = None;
                                let (name, _, _) = split_fx_string(legacy_string.to_string());
                                Some(match fx_legacy_param[i].take() {
                                    Some(p) => format!("{name};{p}"),
                                    None => legacy_string.to_string(),
                                })
                            } else {
                                fx_string[i].take()
                            };

                            if let Some(effect_string) = effect_string {
                                let (name, param_1, param_2) = split_fx_string(effect_string);

                                let v = new_chart
                                    .audio
                                    .audio_effect
                                    .fx
                                    .long_event
                                    .entry(ksh_effect_name(&name))
                                    .or_insert_with(|| [vec![], vec![]]);

                                let params = [
                                    (PLACEHOLDER_PARAM_1, param_1),
                                    (PLACEHOLDER_PARAM_2, param_2),
                                ]
                                .into_iter()
                                .filter_map(|(k, v)| {
                                    v.filter(|v| !v.is_empty()).map(|v| (k.to_string(), v))
                                })
                                .collect();

                                v[i].push(ByPulseOption(long_y[i + 4], Some(params)))
                            }
                        }

                        if c == b'2' {
                            new_chart.note.fx[i].push(Interval { y, l: 0 })
                        } else if is_long(c) && !is_long(last) {
                            long_y[i + 4] = y;
                        }

                        last_char[i + 4] = c;
                    }

                    //read laser
                    for i in 0..2 {
                        if chars[i + 8] == b'-' && last_char[i + 6] != b'-' {
                            // end laser
                            let v = std::mem::replace(
                                &mut laser_builder[i],
                                LaserSection(0, Vec::new(), 1),
                            );
                            if v.1.is_empty() {
                                return Err(KshReadError {
                                    error: KshReadErrorDetails::EmptyLaserSection,
                                    line: file_line,
                                });
                            }
                            new_chart.note.laser[i].push(v);
                        }
                        if chars[i + 8] != b'-' && chars[i + 8] != b':' && last_char[i + 6] == b'-'
                        {
                            let (a, b) = laser_curves[i];
                            laser_curves[i] = (0.5, 0.5);

                            // new laser
                            laser_builder[i].0 = y;
                            laser_builder[i].1.push(
                                GraphSectionPoint::new(
                                    0,
                                    laser_char_to_value(chars[i + 8], laser_builder[i].2)
                                        .with_line(file_line)?,
                                )
                                .with_curve(a, b),
                            );
                        } else if chars[i + 8] != b':' && chars[i + 8] != b'-' {
                            let (a, b) = laser_curves[i];
                            laser_curves[i] = (0.5, 0.5);

                            // new point
                            laser_builder[i].1.push(
                                GraphSectionPoint::new(
                                    y - laser_builder[i].0,
                                    laser_char_to_value(chars[i + 8], laser_builder[i].2)
                                        .with_line(file_line)?,
                                )
                                .with_curve(a, b),
                            );
                        }

                        last_char[i + 6] = chars[i + 8];
                    }

                    if chars.len() > 12 {
                        parse_lane_spin(&mut new_chart, y, chars);
                    }

                    y += ticks_per_line;
                } else if line.starts_with('#') {
                    // Parse custom effect definitions
                    let mut data = line.split_whitespace();
                    let (Some(defined), Some(name)) = (data.next(), data.next()) else {
                        continue;
                    };
                    let data = data.collect::<Vec<_>>().join(" ");

                    let mut data = data
                        .split(';')
                        .filter_map(|x| x.split_once('='))
                        .map(|(k, v)| (ksh_param_name(k), v))
                        .collect::<HashMap<_, _>>();

                    if let Some(Ok(mut t)) = data.remove("type").map(AudioEffect::try_from) {
                        // User-defined SideChain effects in KSH charts older than v200
                        // default to a release time of 1/16
                        if behavior_ver < 200 && matches!(t, AudioEffect::Sidechain(_)) {
                            t = t.derive("release_time", "1/16");
                        }
                        for (key, param) in data.into_iter() {
                            t = t.derive(&key, param)
                        }

                        match defined {
                            "#define_fx" => new_chart
                                .audio
                                .audio_effect
                                .fx
                                .def
                                .push((name.to_owned(), t)),
                            "#define_filter" => new_chart
                                .audio
                                .audio_effect
                                .laser
                                .def
                                .push((name.to_owned(), t)),
                            _ => {}
                        };
                    }
                } else if let Some(comment) = line.strip_prefix("//") {
                    new_chart
                        .editor
                        .get_or_insert_default()
                        .comment
                        .push((y, comment.to_string()));
                } else if let Some((line_prop, line_value)) = line.split_once('=') {
                    let mut line_value = line_value.to_string();

                    match line_prop {
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
                                    (KSON_RESOLUTION * 4 * num / den) / line_count.max(1);
                            }
                            match new_chart.beat.time_sig.last_mut() {
                                Some(last) if last.0 == sig_idx => last.1 = new_sig,
                                _ => new_chart.beat.time_sig.push((sig_idx, new_sig)),
                            }
                        }
                        "t" => {
                            let bpm = line_value.parse().with_line(file_line)?;
                            match new_chart.beat.bpm.last_mut() {
                                Some(last) if last.0 == y => last.1 = bpm,
                                _ => new_chart.beat.bpm.push((y, bpm)),
                            }
                        }
                        "stop" => {
                            let length = line_value.parse::<u32>().with_line(file_line)?;
                            new_chart
                                .beat
                                .stop
                                .push((y, length * 4 * KSON_RESOLUTION / 192));
                        }
                        "laserrange_l" => {
                            line_value.truncate(1);
                            laser_builder[0].2 = line_value.parse().with_line(file_line)?;
                        }
                        "laserrange_r" => {
                            line_value.truncate(1);
                            laser_builder[1].2 = line_value.parse().with_line(file_line)?;
                        }
                        "zoom_bottom" | "zoom_top" | "zoom_side" | "center_split"
                        | "rotation_deg" | "scroll_speed" => {
                            let (mut v, mut vf) =
                                parse_ksh_zoom_values(&line_value).with_line(file_line)?;
                            if line_prop.starts_with("zoom_") {
                                v = v.clamp(-zoom_limit, zoom_limit);
                                vf = vf.map(|vf| vf.clamp(-zoom_limit, zoom_limit));
                            }
                            let body = &mut new_chart.camera.cam.body;
                            let graph = match line_prop {
                                "zoom_bottom" => &mut body.zoom_bottom,
                                "zoom_top" => &mut body.zoom_top,
                                "zoom_side" => &mut body.zoom_side,
                                "center_split" => &mut body.center_split,
                                "rotation_deg" => &mut body.rotation_deg,
                                _ => &mut new_chart.beat.scroll_speed,
                            };
                            graph.push(GraphPoint {
                                y,
                                v,
                                vf,
                                a: 0.5,
                                b: 0.5,
                            })
                        }
                        "zoom_bottom_curve" | "zoom_top_curve" | "zoom_side_curve"
                        | "center_split_curve" | "rotation_deg_curve" | "scroll_speed_curve"
                        | "tilt_curve" => {
                            let key = match line_prop {
                                "zoom_bottom_curve" => "zoom_bottom",
                                "zoom_top_curve" => "zoom_top",
                                "zoom_side_curve" => "zoom_side",
                                "center_split_curve" => "center_split",
                                "rotation_deg_curve" => "rotation_deg",
                                "scroll_speed_curve" => "scroll_speed",
                                _ => "tilt",
                            };
                            graph_curves.entry(key).or_default().push((
                                y,
                                parse_ksh_curve_value(&line_value).with_line(file_line)?,
                            ));
                        }
                        "chokkakuvol" => new_chart.audio.key_sound.laser.vol.push((
                            y,
                            line_value
                                .parse::<f64>()
                                .with_line(file_line)?
                                .div(100.0)
                                .clamp(0.0, 1.0),
                        )),
                        "chokkakuse" => {
                            let name = match line_value.as_str() {
                                "up" => "slam_up".to_string(),
                                "down" => "slam_down".to_string(),
                                "swing" => "slam_swing".to_string(),
                                "mute" => "slam_mute".to_string(),
                                v => v.to_string(),
                            };
                            new_chart
                                .audio
                                .key_sound
                                .laser
                                .slam_event
                                .entry(name)
                                .or_default()
                                .push(y);
                        }
                        "pfiltergain" => {
                            let gain = line_value.parse::<f64>().with_line(file_line)?.div(100.0);
                            new_chart
                                .audio
                                .audio_effect
                                .laser
                                .legacy
                                .get_or_insert_default()
                                .filter_gain
                                .push((y, gain));
                        }
                        "fx-l" => {
                            fx_string[0] = Some(line_value);
                        }
                        "fx-r" => {
                            fx_string[1] = Some(line_value);
                        }
                        "fx-l_param1" => {
                            fx_legacy_param[0] = Some(line_value);
                        }
                        "fx-r_param1" => {
                            fx_legacy_param[1] = Some(line_value);
                        }
                        "tilt" => parse_tilt(&mut new_chart.camera, y, &line_value)
                            .with_line(file_line)?,
                        "filtertype" => {
                            new_chart
                                .audio
                                .audio_effect
                                .laser
                                .pulse_event
                                .entry(ksh_effect_name(&line_value))
                                .or_default()
                                .push(y);
                        }
                        "laser_l_curve" => {
                            laser_curves[0] =
                                parse_ksh_curve_value(&line_value).with_line(file_line)?
                        }
                        "laser_r_curve" => {
                            laser_curves[1] =
                                parse_ksh_curve_value(&line_value).with_line(file_line)?
                        }
                        "fx-l_se" | "fx-r_se" => {
                            let sound_info = &mut new_chart.audio.key_sound.fx;
                            let (file, volume) = &line_value
                                .rsplit_once(';')
                                .unwrap_or_else(|| (&line_value, "100"));
                            let volume: i32 = volume.parse().with_line(file_line)?;

                            let side_idx = if line_prop.chars().nth(3) == Some('l') {
                                0
                            } else {
                                1
                            };

                            let entries =
                                &mut sound_info.chip_event.entry(file.to_string()).or_default()
                                    [side_idx];

                            entries.push(ByPulseOption(
                                y,
                                Some(KeySoundInvokeFX {
                                    vol: volume as f64 / 100.0,
                                }),
                            ));
                        }
                        v => {
                            let parts = v.split(':').collect::<Vec<_>>();
                            let param_change = match parts.as_slice() {
                                ["fx", _, _] => {
                                    Some(&mut new_chart.audio.audio_effect.fx.param_change)
                                }
                                ["filter", _, _] => {
                                    Some(&mut new_chart.audio.audio_effect.laser.param_change)
                                }
                                _ => None,
                            };

                            if let Some(param_change) = param_change {
                                let param = param_change
                                    .entry(ksh_effect_name(parts[1]))
                                    .or_default();
                                let param = param.entry(ksh_param_name(parts[2])).or_default();
                                param.push((y, line_value));
                            } else {
                                unknown
                                    .option
                                    .entry(v.to_string())
                                    .or_default()
                                    .push((y, line_value));
                            }
                        }
                    }
                } else if !line.trim().is_empty() {
                    unknown.line.push((y, line.to_string()));
                }
            }
            measure_index += 1;
        }
        //set slams
        for i in 0..2 {
            for section in &mut new_chart.note.laser[i] {
                let mut iter = section.1.iter_mut();
                let mut for_removal: HashSet<u32> = HashSet::new();
                let mut prev = iter
                    .next()
                    .ok_or(KshReadErrorDetails::EmptyLaserSection)
                    .with_line(usize::MAX)?;
                for next in iter {
                    if (next.ry - prev.ry) <= (KSON_RESOLUTION / 8)
                        && (prev.v - next.v).abs() > f64::EPSILON
                    {
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

        // Set curve "slams" and apply curves
        {
            let body = &mut new_chart.camera.cam.body;
            for (key, graph) in [
                ("zoom_bottom", &mut body.zoom_bottom),
                ("zoom_top", &mut body.zoom_top),
                ("zoom_side", &mut body.zoom_side),
                ("center_split", &mut body.center_split),
                ("rotation_deg", &mut body.rotation_deg),
                ("scroll_speed", &mut new_chart.beat.scroll_speed),
            ] {
                consolidate_graph(graph);
                for (y, (a, b)) in graph_curves.remove(key).unwrap_or_default() {
                    if let Ok(i) = graph.binary_search_by_key(&y, |p| p.y) {
                        graph[i].a = a;
                        graph[i].b = b;
                    }
                }
            }

            for (y, curve) in graph_curves.remove("tilt").unwrap_or_default() {
                if let Ok(i) = new_chart.camera.tilt.binary_search_by_key(&y, |t| t.0) {
                    let t = &mut new_chart.camera.tilt[i].1;
                    *t = match *t {
                        TiltValue::ManualPoint(v) => TiltValue::ManualCurve(v, curve),
                        TiltValue::ManualInstant(v, vf) => {
                            TiltValue::ManualCurveInstant((v, vf), curve)
                        }
                        other => other,
                    };
                }
            }
        }

        // Spec defaults
        if new_chart.beat.scroll_speed.is_empty() {
            new_chart.beat.scroll_speed.push(GraphPoint {
                y: 0,
                v: 1.0,
                vf: None,
                a: 0.5,
                b: 0.5,
            });
        }
        if new_chart.beat.time_sig.first().is_none_or(|t| t.0 != 0) {
            new_chart.beat.time_sig.insert(0, (0, TimeSignature(4, 4)));
        }
        if new_chart.beat.bpm.first().is_none_or(|b| b.0 != 0) {
            let first = new_chart.beat.bpm.first().map(|b| b.1).unwrap_or(120.0);
            new_chart.beat.bpm.insert(0, (0, first));
        }

        if !comments.is_empty() {
            let editor = new_chart.editor.get_or_insert_default();
            comments.append(&mut editor.comment);
            editor.comment = comments;
        }
        if !unknown.is_empty() {
            new_chart.compat.get_or_insert_default().ksh_unknown = unknown;
        }

        // set up effect events
        {
            let effects = &mut new_chart.audio.audio_effect;
            let defs = &effects.fx.def;
            for (name, events) in effects.fx.long_event.iter_mut() {
                let effect = AudioEffect::resolve(defs, name);

                for ele in events.iter_mut().flatten() {
                    let Some(event) = ele.1.as_mut() else {
                        continue;
                    };

                    match &effect {
                        Some(effect) => convert_params(effect, event),
                        None => event.clear(),
                    }

                    if event.is_empty() {
                        ele.1 = None;
                    }
                }
            }
        }

        Ok(new_chart)
    }

    //TODO: Write optimized charts using lcm, also ksm doesn't seem to like resolution > 48
    pub fn to_ksh<W>(&self, out: W) -> Result<(), KshWriteError>
    where
        W: std::io::Write,
    {
        let mut w = BufWriter::new(out);

        //Meta
        {
            // KSM detects UTF-8 by the BOM
            w.write_all("\u{feff}".as_bytes())?;
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
            let bgm = self.audio.bgm.clone();
            writeln!(&mut w, "m={}\r", bgm.filename)?;
            writeln!(&mut w, "mvol={}\r", (bgm.vol * 100.0).round() as i64)?;
            writeln!(&mut w, "o={}\r", bgm.offset)?;
            writeln!(&mut w, "po={}\r", bgm.preview.offset)?;

            let bpm_cmp = |a: &&(u32, f64), b: &&(u32, f64)| a.1.total_cmp(&b.1);

            let min_bpm = self
                .beat
                .bpm
                .iter()
                .min_by(bpm_cmp)
                .map(|x| x.1)
                .unwrap_or_default();
            let max_bpm = self
                .beat
                .bpm
                .iter()
                .max_by(bpm_cmp)
                .map(|x| x.1)
                .unwrap_or_default();
            if min_bpm == max_bpm {
                writeln!(&mut w, "t={}\r", min_bpm)?;
            } else {
                writeln!(&mut w, "t={:.1}-{:.1}\r", min_bpm, max_bpm)?;
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
                let sig = self.beat.time_sig[i];

                writeln!(&mut w, "beat={}/{}\r", sig.1 .0, sig.1 .1)?;
            }

            let next_measure_tick = self.measure_to_tick(measure + 1);
            let slam_distance = KSON_RESOLUTION / 8;
            for y in measure_tick..next_measure_tick {
                //Tick events
                {
                    //BPM
                    if let Ok(b) = self.beat.bpm.binary_search_by(|f| f.0.cmp(&y)) {
                        if (y > 0 && self.beat.bpm.len() == 1) || self.beat.bpm.len() > 1 {
                            let bpm = self.beat.bpm[b];
                            writeln!(&mut w, "t={}\r", bpm.1)?;
                        }
                    }

                    //Laser width
                    if let Ok(b) = self.note.laser[0].binary_search_by(|f| f.0.cmp(&y)) {
                        let l = &self.note.laser[0][b];
                        if l.2 == 2 {
                            writeln!(&mut w, "laserrange_l=2x\r")?;
                        }
                    }
                    if let Ok(b) = self.note.laser[1].binary_search_by(|f| f.0.cmp(&y)) {
                        let l = &self.note.laser[1][b];
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
                            let note = l[i];
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
                            let note = l[i];
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
                            let section = &l[i];
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
                                        let point = s.1[point_i];
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
                                            let point =
                                                s.1.get(point_i - 1)
                                                    .expect("Failed to get previous laser point");
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
                                                match (Some(point.a), Some(point.b)) {
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
                                                                s.value_at(y as f64).expect("Tried to get value outside of laser"),
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
                                            let point = s.1[point_i - 1];
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

fn parse_tilt(
    tilt: &mut camera::CameraInfo,
    y: u32,
    line_value: &str,
) -> Result<(), KshReadErrorDetails> {
    let value = match line_value {
        "normal" => Ok(NamedTiltValue::Normal),
        "bigger" | "big" => Ok(NamedTiltValue::Bigger),
        "biggest" => Ok(NamedTiltValue::Biggest),
        "keep_normal" => Ok(NamedTiltValue::KeepNormal),
        "keep_bigger" | "keep" => Ok(NamedTiltValue::KeepBigger),
        "keep_biggest" => Ok(NamedTiltValue::KeepBiggest),
        "zero" => Ok(NamedTiltValue::Zero),
        v => Err(v.parse::<f64>()?),
    };

    if let Some(last) = tilt.tilt.last_mut() {
        if last.0 == y {
            last.1 = match (last.1, value) {
                (TiltValue::Named(_), Ok(named)) => TiltValue::Named(named),
                (TiltValue::Named(_), Err(v)) => TiltValue::ManualPoint(v),
                (TiltValue::ManualPoint(v), Ok(named)) => TiltValue::ManualToNamedInstant(v, named),
                (TiltValue::ManualPoint(v), Err(vf)) => TiltValue::ManualInstant(v, vf),
                (TiltValue::ManualInstant(_, vf), Ok(named)) => {
                    TiltValue::ManualToNamedInstant(vf, named)
                }
                (TiltValue::ManualInstant(v, _), Err(vff)) => TiltValue::ManualInstant(v, vff),
                (TiltValue::ManualToNamedInstant(v, _), Ok(new_named)) => {
                    TiltValue::ManualToNamedInstant(v, new_named)
                }
                (TiltValue::ManualToNamedInstant(v, _), Err(vf)) => TiltValue::ManualInstant(v, vf),
                _ => {
                    return Err(KshReadErrorDetails::InvalidCurveValue(
                        "Tilt curves not supported in ksh".to_string(),
                    ))
                }
            };

            return Ok(());
        }
    }

    tilt.tilt.push((
        y,
        match value {
            Ok(named) => TiltValue::Named(named),
            Err(value) => TiltValue::ManualPoint(value),
        },
    ));

    Ok(())
}

fn convert_params(effect: &AudioEffect, params: &mut Dict<String>) {
    let p1 = params.remove(PLACEHOLDER_PARAM_1).unwrap_or_else(|| {
        match effect {
            AudioEffect::Retrigger(_) => "8",
            AudioEffect::Gate(_) => "4",
            AudioEffect::PitchShift(_) => "12",
            AudioEffect::BitCrusher(_) => "5",
            AudioEffect::Wobble(_) => "12",
            AudioEffect::Tapestop(_) => "50",
            AudioEffect::Echo(_) => "4",
            _ => "",
        }
        .to_string()
    });
    let p2 = params.remove(PLACEHOLDER_PARAM_2).unwrap_or_else(|| {
        match effect {
            AudioEffect::Echo(_) => "60",
            _ => "0",
        }
        .to_string()
    });

    match effect {
        AudioEffect::Retrigger(_) | AudioEffect::Gate(_) | AudioEffect::Wobble(_) => {
            if !p1.is_empty() && p1.chars().all(|x| x.is_ascii_digit()) {
                params.insert("wave_length".to_string(), format!("1/{p1}"));
            }
        }
        AudioEffect::PitchShift(_) => {
            params.insert("pitch".to_string(), p1);
        }
        AudioEffect::BitCrusher(_) => {
            params.insert("reduction".to_string(), format!("{p1}samples"));
        }
        AudioEffect::Tapestop(_) => {
            params.insert("speed".to_string(), format!("{p1}%"));
        }
        AudioEffect::Echo(_) => {
            if !p1.is_empty() && p1.chars().all(|x| x.is_ascii_digit()) {
                params.insert("wave_length".to_string(), format!("1/{p1}"));
            }
            params.insert("feedback_level".to_string(), format!("{p2}%"));
        }
        _ => {}
    }
}

fn consolidate_graph(graph: &mut Vec<GraphPoint>) {
    let mut i = 0;
    while i < graph.len().saturating_sub(1) {
        let window = &mut graph[i..=i + 1];

        if window[0].y == window[1].y {
            window[0].vf = Some(window[1].v);
            graph.remove(i + 1);
        } else {
            i += 1
        }
    }
}

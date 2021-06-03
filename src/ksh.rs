use crate::*;
use anyhow::ensure;
use anyhow::Result;
pub trait Ksh {
    fn from_ksh(data: &str) -> Result<crate::Chart>;
    fn to_ksh<W>(&self, out: W) -> Result<()>
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
fn laser_char_to_value(value: u8) -> Result<f64> {
    let v = find_laser_char(value);
    ensure!(v != u8::MAX, "Invalid laser char: '{}'", value as char);
    Ok(v as f64 / 50.0)
}

fn parse_ksh_zoom_values(data: &str) -> Result<(f64, Option<f64>)> {
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

impl Ksh for crate::Chart {
    fn from_ksh(data: &str) -> Result<crate::Chart> {
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
                    if let Ok(v) = value.parse::<f64>() {
                        new_chart.beat.bpm.push(ByPulse { y: 0, v })
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
        let mut last_char: [u8; 8] = [b'0'; 8];
        last_char[6] = b'-';
        last_char[7] = b'-';

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
                                LaserSection {
                                    y: 0,
                                    v: Vec::new(),
                                    wide: 1,
                                },
                            );
                            new_chart.note.laser[i].push(v);
                        }
                        if chars[i + 8] != b'-' && chars[i + 8] != b':' && last_char[i + 6] == b'-'
                        {
                            // new laser
                            laser_builder[i].y = y;
                            laser_builder[i].v.push(GraphSectionPoint::new(
                                0,
                                laser_char_to_value(chars[i + 8] as u8)?,
                            ));
                        } else if chars[i + 8] != b':' && chars[i + 8] != b'-' {
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

    fn to_ksh<W>(&self, _out: W) -> Result<()>
    where
        W: std::io::Write,
    {
        todo!()
    }
}

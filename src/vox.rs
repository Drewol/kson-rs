use thiserror::Error;

use crate::ByMeasureIdx;
use crate::ByPulse;
use crate::Chart;
use crate::GraphSectionPoint;
use crate::Interval;
use crate::LaserSection;
use crate::TimeSignature;

#[derive(Debug, Error)]
pub enum VoxReadError {
    #[error("Unknown Track Identifier: '{0}'")]
    UnknownTrackId(String),
    #[error("Failed to parse value: '{0}'")]
    ParseError(#[from] std::string::ParseError),
    #[error("Failed to parse value: '{0}'")]
    ParseFloatError(#[from] std::num::ParseFloatError),
    #[error("Failed to parse value: '{0}'")]
    ParseIntError(#[from] std::num::ParseIntError),
    #[error("Failed to parse line values: '{0}'")]
    LineParseError(String),
    #[error("Failed to parse VOX time: '{0}'")]
    TimeParseError(String),
    #[error("Unsupported VOX version: {0}")]
    UnsupportedVersionError(u32),
    #[error("Unknown laser node type: {0}")]
    UnknownLaserNodeError(i32),
}

#[derive(Debug, Error)]
pub enum VoxWriteError {}

pub trait Vox {
    fn from_vox(data: &str) -> Result<crate::Chart, VoxReadError>;
    fn to_vox<W>(&self, out: W) -> Result<(), VoxWriteError>
    where
        W: std::io::Write;
}

#[inline]
fn is_not_end(line: &&str) -> bool {
    *line != "#END"
}

#[inline]
fn is_not_comment(line: &&str) -> bool {
    !line.starts_with("//")
}

#[inline]
fn split_data_line(line: &str) -> Vec<&str> {
    let comment_idx = line.find("//").unwrap_or(line.len());
    let uncommented = &line[0..comment_idx];
    uncommented.split('\t').filter(|s| !s.is_empty()).collect()
}

#[inline]
fn time_sig_accumulator(
    mut accu: ByMeasureIdx<TimeSignature>,
    line_data: Vec<&str>,
) -> Result<ByMeasureIdx<TimeSignature>, VoxReadError> {
    let measure = line_data
        .get(0).and_then(|v| v.split(',').next().map(|i| i.parse::<u32>()));
    if let Some(Ok(m)) = measure {
        accu.push((m - 1, TimeSignature(line_data.get(1).unwrap_or(&"").parse()?, line_data.get(2).unwrap_or(&"").parse()?)));
        Ok(accu)
    } else {
        Err(VoxReadError::LineParseError(line_data.join(", ")))
    }
}

#[inline]
fn tick_from_vox(vox_time: &str, chart: &Chart) -> Result<u32, VoxReadError> {
    let mut time_parts = vox_time.split(',');
    let (measure, beat, ticks): (u32, u32, u32) =
        match (time_parts.next(), time_parts.next(), time_parts.next()) {
            (Some(m), Some(b), Some(t)) => (m.parse()?, b.parse()?, t.parse()?),
            _ => return Err(VoxReadError::TimeParseError(vox_time.to_string())),
        };

    let current_sig = match chart
        .beat
        .time_sig
        .binary_search_by_key(&(measure - 1), |t| t.0)
    {
        Ok(i) => chart.beat.time_sig.get(i).unwrap(),
        Err(i) => chart.beat.time_sig.get(i - 1).unwrap(),
    };

    let tick_per_beat = 192 / current_sig.1.1;
    Ok(chart.measure_to_tick(measure - 1) + tick_per_beat * (beat - 1) + ticks)
}

impl Vox for crate::Chart {
    fn from_vox(data: &str) -> Result<crate::Chart, VoxReadError> {
        let mut data = data.lines();
        let mut chart = crate::Chart::new();

        let mut tracks: [Vec<Vec<&str>>; 8] = Default::default();
        let mut vox_version = 0;
        let mut bpm_info = Vec::new();

        while let Some(line) = data.next() {
            match line {
                "#FORMAT VERSION" => vox_version = data.by_ref().next().unwrap_or("0").parse()?,
                "#BEAT INFO" => {
                    chart.beat.time_sig = data
                        .by_ref()
                        .take_while(is_not_end)
                        .filter(is_not_comment)
                        .map(split_data_line)
                        .try_fold(Vec::new(), time_sig_accumulator)?;
                }
                "#BPM INFO" => {
                    bpm_info = data
                        .by_ref()
                        .take_while(is_not_end)
                        .filter(is_not_comment)
                        .map(split_data_line)
                        .collect()
                }
                "#TAB EFFECT INFO" => {}
                "#FXBUTTON EFFECT INFO" => {}
                "#TAB PARAM ASSIGN INFO" => {}
                "#SPCONTROLER" => {} //Camera
                "#TRACK AUTO TAB" | "#TRACK ORIGINAL L" | "#TRACK ORIGINAL R" => {}
                track if track.starts_with("#TRACK") => {
                    let tracknum = match track.chars().filter_map(|c| c.to_digit(10)).next() {
                        Some(c) => c,
                        None => return Err(VoxReadError::UnknownTrackId(track.to_string())),
                    };
                    if (1..=8).contains(&tracknum) {
                        tracks[tracknum as usize - 1] = data
                            .by_ref()
                            .take_while(is_not_end)
                            .filter(is_not_comment)
                            .map(split_data_line)
                            .collect();
                    } else {
                        return Err(VoxReadError::UnknownTrackId(tracknum.to_string()));
                    }
                }
                _ => (),
            }
        }

        chart.beat.bpm = bpm_info.iter().try_fold(
            Vec::new(),
            |mut bpm, line| -> Result<ByPulse<f64>, VoxReadError> {
                let tick = tick_from_vox(line[0], &chart)?;
                bpm.push((tick, line[1].trim().parse()?));
                Ok(bpm)
            },
        )?;

        for (track_idx, track) in tracks.iter().enumerate() {
            if track_idx == 0 || track_idx == 7 {
                //laser tracks
                let (lasers, _, _) = track.iter().try_fold(
                    (Vec::new(), LaserSection (0, Vec::new(), 0), 300),
                    |(mut lasers, mut current_section, mut last_vox_v),
                     line|
                     -> Result<(Vec<LaserSection>, LaserSection, i32), VoxReadError> {
                        let y = tick_from_vox(line[0], &chart)?;
                        let v = if vox_version < 12 {
                            let vox_v: i32 = line[1].parse()?;
                        let vox_v = if (vox_v - last_vox_v).abs() == 1 {last_vox_v} else {vox_v};
                        last_vox_v = vox_v;
                            vox_v as f64 / 127.0}
                            else {
                                line[1].parse()?
                            };

                        let node_type: i32 = line[2].parse()?;
                        
                        let wide: u8 = if line.len() > 5 {
                            line[5].parse()?
                        }
                        else { //for earlier versions
                            1
                        };
                            
                        match node_type {
                            1 => {//start node
                                current_section = LaserSection(
                                    y,
                                    vec![GraphSectionPoint {
                                        ry: 0,
                                        v,
                                        a: None,
                                        b: None,
                                        vf: None,
                                    }],
                                    wide
                                )
                            }
                            0 | 2 => {//mid node | end node
                                let ry = y - current_section.0;
                                if let Some(last) = current_section.1.last_mut() {
                                    if last.ry == ry {
                                        last.vf = Some(v);
                                    }
                                    else
                                    {
                                        current_section.1.push(GraphSectionPoint {
                                            ry,
                                            v ,
                                            a: None,
                                            b: None,
                                            vf: None,
                                        });
                                    }
                                }
                                else {
                                    unreachable!();
                                }
                            }
                            _ => return Err(VoxReadError::UnknownLaserNodeError(node_type))
                        }
                        if node_type == 2 {
                            let finished_section = std::mem::replace(&mut  current_section,  LaserSection(y, vec![GraphSectionPoint {ry: 0,v: 0.0,a: None,b: None,vf: None,
                                }],
                                wide  
                            ));
                            lasers.push(finished_section);
                        }
                        Ok((lasers, current_section, last_vox_v))
                    },
                )?;
                chart.note.laser[track_idx / 7] = lasers;
            } else {
                let notes = track.iter().try_fold(
                    //TODO: effect index
                    Vec::new(),
                    |mut notes, line| -> Result<Vec<Interval>, VoxReadError> {
                        let y = tick_from_vox(line[0], &chart)?;
                        let l = line[1].parse()?;
                        notes.push(Interval { y, l });
                        Ok(notes)
                    },
                )?;

                match track_idx {
                    1 | 6 => chart.note.fx[track_idx / 6] = notes,
                    2..=5 => chart.note.bt[track_idx - 2] = notes,
                    _ => unreachable!(),
                }
            }
        }

        Ok(chart)
    }

    fn to_vox<W>(&self, _out: W) -> Result<(), VoxWriteError>
    where
        W: std::io::Write,
    {
        todo!()
    }
}

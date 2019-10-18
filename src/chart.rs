extern crate regex;
extern crate serde;
extern crate serde_json;

use self::serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::str::Lines;

#[derive(Serialize, Deserialize)]
pub struct GraphSectionPoint {
    pub ry: u32,
    pub v: f64,
    pub vf: Option<f64>,
    pub a: f64,
    pub b: f64,
}

impl GraphSectionPoint {
    fn new(_ry: u32, _v: f64) -> Self {
        GraphSectionPoint {
            ry: _ry,
            v: _v,
            vf: None,
            a: 0.0,
            b: 0.0,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Interval {
    pub y: u32,
    pub l: u32,
}

#[derive(Serialize, Deserialize)]
pub struct LaserSection {
    pub y: u32,
    pub v: Vec<GraphSectionPoint>,
    pub wide: u8,
}

#[derive(Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize)]
pub struct DifficultyInfo {
    pub name: String,
    pub short_name: String,
    pub idx: u8,
}

#[derive(Serialize, Deserialize)]
pub struct MetaInfo {
    pub title: String,
    pub title_translit: String,
    pub subtitle: String,
    pub artist: String,
    pub artist_translit: String,
    pub chart_author: String,
    pub difficulty: DifficultyInfo,
    pub level: u8,
    pub disp_bpm: String,
    pub std_bpm: f64,
    pub jacket_filename: String,
    pub jacket_author: String,
    pub information: String,
}

impl DifficultyInfo {
    fn new() -> DifficultyInfo {
        DifficultyInfo {
            name: String::new(),
            short_name: String::new(),
            idx: 0,
        }
    }
}

impl MetaInfo {
    fn new() -> MetaInfo {
        MetaInfo {
            title: String::new(),
            title_translit: String::new(),
            subtitle: String::new(),
            artist: String::new(),
            artist_translit: String::new(),
            chart_author: String::new(),
            difficulty: DifficultyInfo::new(),
            level: 1,
            disp_bpm: String::new(),
            std_bpm: std::f64::NAN,
            jacket_filename: String::new(),
            jacket_author: String::new(),
            information: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct DoubleEvent {
    pub y: u32,
    pub v: f64,
}

#[derive(Serialize, Deserialize)]
pub struct BeatInfo {
    pub bpm: Vec<DoubleEvent>,
    pub resolution: u32,
}

impl BeatInfo {
    fn new() -> Self {
        BeatInfo {
            bpm: Vec::new(),
            resolution: 240,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Chart {
    pub meta: MetaInfo,
    pub note: NoteInfo,
    pub beat: BeatInfo,
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
            i = i + 1;
        }
    }
    Err(String::from(format!(
        "Invalid laser char: '{}'",
        value as char
    )))
}

impl Chart {
    pub fn new() -> Self {
        Chart {
            meta: MetaInfo::new(),
            note: NoteInfo::new(),
            beat: BeatInfo::new(),
        }
    }

    pub fn from_ksh(path: &String) -> Result<Chart, String> {
        let mut new_chart = Chart::new();
        let data = fs::read_to_string(path);
        if data.is_err() {
            match data.err() {
                Some(error) => return Err(format!("{}", error)),
                None => return Err(String::from("Unknown error.")),
            }
        }

        let data = data.unwrap();
        let data = &data[3..]; //Something about BOM(?)
        let mut parts: Vec<&str> = data.split("\n--").collect();
        let meta = (parts.first().unwrap()).lines();
        for line in meta {
            let line_data: Vec<&str> = line.split("=").collect();
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
                    if !value.contains("-") {
                        new_chart.beat.bpm.push(DoubleEvent {
                            y: 0,
                            v: value.parse().unwrap_or_else(|e| {
                                println!("{}", e);
                                panic!(e)
                            }),
                        })
                    }
                }
                _ => (),
            }
        }

        parts.remove(0);
        let mut y: u32 = 0;

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
            let note_regex = regex::Regex::new("[0-2]{4}\\|").unwrap();
            let line_count = measure.lines().filter(|x| note_regex.is_match(x)).count() as u32;
            if line_count == 0 {
                continue;
            }
            let ticks_per_line = (new_chart.beat.resolution * 4) / line_count; //TODO: use time signature
            for line in measure_lines {
                if note_regex.is_match(line) {
                    //read bt
                    let chars: Vec<char> = line.chars().collect();
                    for i in 0..4 {
                        if chars[i] == '1' {
                            new_chart.note.bt[i].push(Interval { y: y, l: 0 });
                        } else if chars[i] == '2' && last_char[i] != '2' {
                            long_y[i] = y;
                        } else if chars[i] != '2' && last_char[i] == '2' {
                            new_chart.note.bt[i].push(Interval {
                                y: long_y[i],
                                l: y - long_y[i],
                            });
                        }

                        last_char[i] = chars[i];
                    }

                    //read fx
                    for i in 0..2 {
                        if chars[i + 5] == '2' {
                            new_chart.note.fx[i].push(Interval { y: y, l: 0 })
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
                            laser_builder[i] = LaserSection {
                                y: y,
                                v: Vec::new(),
                                wide: 1,
                            };
                            laser_builder[i].v.push(GraphSectionPoint::new(
                                0,
                                laser_char_to_value(chars[i + 8] as u8).unwrap(),
                            ));
                        } else if chars[i + 8] != ':' && last_char[i + 6] == ':' {
                            // new point
                            laser_builder[i].v.push(GraphSectionPoint::new(
                                y - laser_builder[i].y,
                                laser_char_to_value(chars[i + 8] as u8).unwrap(),
                            ));
                        }

                        last_char[i + 6] = chars[i + 8];
                    }

                    y = y + ticks_per_line;
                }

                //set slams
                for i in 0..2 {
                    for section in &mut new_chart.note.laser[i] {
                        let mut iter = section.v.iter_mut();
                        let mut for_removal: HashSet<u32> = HashSet::new();
                        let mut prev = iter.next().unwrap();
                        loop {
                            let n = iter.next();
                            match n {
                                None => break,
                                _ => (),
                            }
                            let next = n.unwrap();

                            if next.ry - prev.ry <= new_chart.beat.resolution / 8 {
                                prev.vf = Some(next.v);
                                for_removal.insert(next.ry);
                            }

                            prev = next;
                        }
                        section.v.retain(|p| !for_removal.contains(&p.ry));
                    }
                }
            }
        }

        return Ok(new_chart);
    }
}

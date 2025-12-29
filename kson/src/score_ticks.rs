use std::{cell::RefCell, rc::Rc};

use crate::*;

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct KeySoundEvent {
    pub file: i32,
    pub volume: f64,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ScoreTick {
    Laser {
        lane: usize,
        pos: f64,
    },
    Slam {
        lane: usize,
        start: f64,
        end: f64,
    },
    Chip {
        lane: usize,
        key_sound: Option<KeySoundEvent>,
    },
    Hold {
        lane: usize,
        start_tick: u32,
    },
}

impl ScoreTick {
    pub fn lane(&self) -> usize {
        match self {
            ScoreTick::Laser { lane, pos: _ } => *lane,
            ScoreTick::Slam {
                lane,
                start: _,
                end: _,
            } => *lane,
            ScoreTick::Chip { lane, .. } => *lane,
            ScoreTick::Hold { lane, .. } => *lane,
        }
    }

    pub fn global_lane(&self) -> usize {
        match self {
            ScoreTick::Laser { .. } | ScoreTick::Slam { .. } => self.lane() + 6,
            _ => self.lane(),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct PlacedScoreTick {
    pub y: u32,
    pub tick: ScoreTick,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ScoreTickSummary {
    pub chip_count: u32,
    pub hold_count: u32,
    pub laser_count: u32,
    pub slam_count: u32,
    pub total: u32,
}

pub trait ScoreTicker {
    fn summary(&self) -> ScoreTickSummary;
    fn get_combo_at(&self, y: u32) -> u32;
}

fn get_hold_step_at(y: u32, chart: &Chart) -> u32 {
    if chart.bpm_at_tick(y) > 255.0 {
        KSON_RESOLUTION / 2
    } else {
        KSON_RESOLUTION / 4
    }
}

fn ticks_from_interval(
    interval: &Interval,
    lane: usize,
    chart: &Chart,
    key_sound_map: Rc<RefCell<HashMap<i32, String>>>,
) -> Vec<PlacedScoreTick> {
    let mut key_sound_map = key_sound_map.borrow_mut();
    if interval.l == 0 {
        // Find keysound
        let key_sound = if lane < 4 {
            None
        } else {
            if let Some((file, event_idx)) =
                chart
                    .audio
                    .key_sound
                    .fx
                    .chip_event
                    .iter()
                    .find_map(|(file, sides)| {
                        sides[lane - 4]
                            .binary_search_by_key(&interval.y, |v| v.0)
                            .ok()
                            .map(|s| (file, s))
                    })
            {
                let (_, event) = &chart.audio.key_sound.fx.chip_event[file][lane - 4][event_idx];

                if let Some((key, _)) = key_sound_map.iter().find(|(_, value)| *value == file) {
                    Some(KeySoundEvent {
                        file: *key,
                        volume: event.vol,
                    })
                } else {
                    let next_idx = key_sound_map.len() as i32;
                    key_sound_map.insert(next_idx, file.clone());
                    Some(KeySoundEvent {
                        file: next_idx,
                        volume: event.vol,
                    })
                }
            } else {
                None
            }
        };

        vec![PlacedScoreTick {
            y: interval.y,
            tick: ScoreTick::Chip { lane, key_sound },
        }]
    } else {
        let mut res = Vec::new();

        let mut y = interval.y;
        let mut step = get_hold_step_at(y, chart);
        y += step;
        y -= y % step;
        while y <= interval.y + interval.l - step {
            res.push(PlacedScoreTick {
                y,
                tick: ScoreTick::Hold {
                    lane,
                    start_tick: interval.y,
                },
            });
            step = get_hold_step_at(y, chart);
            y += step;
        }

        //Ensure holds always have a tick.
        if res.is_empty() {
            res.push(PlacedScoreTick {
                y: interval.y + interval.l / 2,
                tick: ScoreTick::Hold {
                    lane,
                    start_tick: interval.y,
                },
            })
        }

        res
    }
}

fn get_if_slam(point: Option<&GraphSectionPoint>, lane: usize, y: u32) -> Option<PlacedScoreTick> {
    if let Some(s) = point {
        s.vf.map(|vf| PlacedScoreTick {
            y: y + s.ry,
            tick: ScoreTick::Slam {
                lane,
                end: vf,
                start: s.v,
            },
        })
    } else {
        None
    }
}

fn ticks_from_laser_section(
    section: &LaserSection,
    lane: usize,
    chart: &Chart,
) -> Vec<PlacedScoreTick> {
    let mut res = Vec::new();

    let mut first = true;
    for se in section.1.windows(2) {
        let s = se[0];
        let e = se[1];
        if let Some(t) = get_if_slam(Some(&s), lane, section.0) {
            res.push(t)
        }

        let mut y = section.0 + s.ry;
        let mut step = get_hold_step_at(y, chart);
        if s.vf.is_some() || first {
            y += step;
        }
        y -= y % step;
        while y <= section.0 + e.ry - step {
            if match res.last() {
                Some(s) => s.y == y,
                None => false,
            } {
                step = get_hold_step_at(y, chart);
                y += step;
                continue;
            }

            res.push(PlacedScoreTick {
                y,
                tick: ScoreTick::Laser {
                    lane,
                    pos: section.value_at(y as f64).unwrap_or_default(),
                },
            });
            step = get_hold_step_at(y, chart);
            y += step;
        }
        first = false;
    }

    if let Some(t) = get_if_slam(section.1.last(), lane, section.0) {
        res.push(t);
    }

    //ensure there's always one tick
    if res.is_empty() {
        assert!(section.1.len() >= 2);
        let y = section.0 + section.1.last().map(|s| s.ry / 2).unwrap_or_default();

        res.push(PlacedScoreTick {
            y,
            tick: ScoreTick::Laser {
                lane,
                pos: section.value_at(y as f64).unwrap_or_default(),
            },
        })
    }

    res
}

pub struct ScoreTicks {
    pub ticks: Vec<PlacedScoreTick>,
    pub key_sound_map: HashMap<i32, String>,
}

pub fn generate_score_ticks(chart: &Chart) -> ScoreTicks {
    let mut ticks = Vec::new();
    let key_sound_map = Rc::new(RefCell::new(HashMap::new()));

    ticks.append(
        &mut chart
            .note
            .bt
            .iter()
            .enumerate()
            .flat_map(|(lane, l)| {
                let ksm = key_sound_map.clone();
                l.iter()
                    .map(move |i| ticks_from_interval(i, lane, chart, ksm.clone()))
            })
            .flatten()
            .collect(),
    );
    ticks.append(
        &mut chart
            .note
            .fx
            .iter()
            .enumerate()
            .flat_map(|(lane, l)| {
                let ksm = key_sound_map.clone();
                l.iter()
                    .map(move |i| ticks_from_interval(i, lane + 4, chart, ksm.clone()))
            })
            .flatten()
            .collect(),
    );
    ticks.append(
        &mut chart
            .note
            .laser
            .iter()
            .enumerate()
            .flat_map(|(lane, l)| {
                l.iter()
                    .map(move |s| ticks_from_laser_section(s, lane, chart))
            })
            .flatten()
            .collect(),
    );

    ticks.sort_by(|pa, pb| pa.y.cmp(&pb.y));
    let key_sound_map = Rc::try_unwrap(key_sound_map)
        .expect("Reference kept in closure")
        .into_inner();

    ScoreTicks {
        ticks,
        key_sound_map,
    }
}

impl ScoreTicker for ScoreTicks {
    fn summary(&self) -> ScoreTickSummary {
        let mut res: ScoreTickSummary = Default::default();

        for t in self.ticks.iter() {
            res.total += 1;
            match t.tick {
                ScoreTick::Laser { .. } => res.laser_count += 1,
                ScoreTick::Slam { .. } => res.slam_count += 1,
                ScoreTick::Chip { .. } => res.chip_count += 1,
                ScoreTick::Hold { .. } => res.hold_count += 1,
            }
        }

        res
    }

    fn get_combo_at(&self, y: u32) -> u32 {
        match self.ticks.binary_search_by(|f| f.y.cmp(&y)) {
            Ok(c) => c as u32 + 1,
            Err(c) => c as u32,
        }
    }
}

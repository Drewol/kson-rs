use std::collections::VecDeque;

use anyhow::bail;
use kson::score_ticks::{PlacedScoreTick, ScoreTick};

use super::HitRating;

pub const GAUGE_SAMPLES: usize = 128;

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, Clone, Copy)]
#[repr(u8)]
pub enum GaugeType {
    #[default]
    Normal,
    Hard,
}

impl TryFrom<Gauge> for GaugeType {
    type Error = anyhow::Error;

    fn try_from(value: Gauge) -> Result<Self, Self::Error> {
        match value {
            Gauge::None => bail!("Invalid gauge type"),
            Gauge::Normal { .. } => Ok(Self::Normal),
            Gauge::Hard { .. } => Ok(Self::Hard),
        }
    }
}

impl GaugeType {
    pub const fn fallback_supported(self) -> bool {
        match self {
            GaugeType::Normal => false,
            GaugeType::Hard => true,
        }
    }

    fn gain_rate(self) -> f32 {
        match self {
            GaugeType::Normal => 1.0,
            GaugeType::Hard => 12.0 / 21.0,
        }
    }

    pub fn get_gauge(self, chip_gain: f32, tick_gain: f32) -> Gauge {
        let chip_gain = chip_gain * self.gain_rate();
        let tick_gain = tick_gain * self.gain_rate();
        match self {
            GaugeType::Normal => Gauge::Normal {
                chip_gain,
                tick_gain,
                value: 0.0,
                samples: Box::new([0.0; GAUGE_SAMPLES]),
            },
            GaugeType::Hard => Gauge::Hard {
                chip_gain,
                tick_gain,
                value: 1.0,
                samples: Box::new([0.0; GAUGE_SAMPLES]),
            },
        }
    }
}

#[derive(Debug, Default)]
pub enum Gauge {
    #[default]
    None,
    Normal {
        chip_gain: f32,
        tick_gain: f32,
        value: f32,
        samples: Box<[f32; GAUGE_SAMPLES]>,
    },
    Hard {
        chip_gain: f32,
        tick_gain: f32,
        value: f32,
        samples: Box<[f32; GAUGE_SAMPLES]>,
    },
}

#[derive(Default)]
pub struct Gauges {
    pub active: Gauge,
    fallback: VecDeque<Gauge>,
    failed: Vec<Gauge>,
}

impl Gauges {
    pub const fn new(active: Gauge, fallback: VecDeque<Gauge>) -> Self {
        Self {
            active,
            fallback,
            failed: vec![],
        }
    }

    pub fn is_cleared(&self) -> bool {
        self.active.is_cleared()
    }

    pub fn on_hit(&mut self, rating: HitRating) {
        for ele in self.fallback.iter_mut() {
            ele.on_hit(rating);
        }
        self.active.on_hit(rating);

        if self.active.is_dead() {
            if let Some(fallback) = self.fallback.pop_front() {
                self.failed
                    .push(std::mem::replace(&mut self.active, fallback));
            }
        }
    }

    pub fn update_sample(&mut self, sample: usize) {
        for ele in self.fallback.iter_mut() {
            ele.update_sample(sample);
        }
        self.active.update_sample(sample)
    }
    pub fn is_dead(&self) -> bool {
        self.active.is_dead() && self.fallback.is_empty()
    }
}

const fn tick_is_short(score_tick: PlacedScoreTick) -> bool {
    match score_tick.tick {
        ScoreTick::Laser { lane: _, pos: _ } => false,
        ScoreTick::Slam {
            lane: _,
            start: _,
            end: _,
        } => true,
        ScoreTick::Chip { lane: _ } => true,
        ScoreTick::Hold { .. } => false,
    }
}

fn hard_drain_multiplier(value: f32) -> f32 {
    f32::clamp((0.3 - value).mul_add(-2.0, 1.0), 0.5, 1.0)
}

impl Gauge {
    pub fn gain_rate(&self) -> f32 {
        match self {
            Gauge::None => 1.0,
            Gauge::Normal { .. } => 1.0,
            Gauge::Hard { .. } => 12.0 / 21.0,
        }
    }

    pub const fn miss_drain_percent(&self) -> f32 {
        match self {
            Gauge::None => 0.02,
            Gauge::Normal { .. } => 0.02,
            Gauge::Hard { .. } => 0.09,
        }
    }

    pub fn on_hit(&mut self, rating: HitRating) {
        let short_miss_percent = self.miss_drain_percent();

        match self {
            Gauge::None => {}
            Gauge::Normal {
                chip_gain,
                tick_gain,
                value,
                ..
            } => match rating {
                HitRating::Crit { tick: t, .. } if tick_is_short(t) => *value += *chip_gain,
                HitRating::Crit { .. } => *value += *tick_gain,
                HitRating::Good { .. } => *value += *chip_gain / 3.0, //Only chips can have a "good" rating
                HitRating::Miss { tick: t, .. } if tick_is_short(t) => *value -= short_miss_percent,
                HitRating::Miss { .. } => *value -= short_miss_percent / 4.0,
                HitRating::None => {}
            },
            Gauge::Hard {
                chip_gain,
                tick_gain,
                value,
                ..
            } if *value > 0.0 => match rating {
                HitRating::Crit { tick: t, .. } if tick_is_short(t) => *value += *chip_gain,
                HitRating::Crit { .. } => *value += *tick_gain,
                HitRating::Good { .. } => *value += *chip_gain / 3.0, //Only chips can have a "good" rating
                HitRating::Miss { tick: t, .. } if tick_is_short(t) => {
                    *value -= short_miss_percent * hard_drain_multiplier(*value)
                }
                HitRating::Miss { .. } => {
                    *value -= hard_drain_multiplier(*value) * short_miss_percent / 4.0
                }
                HitRating::None => {}
            },

            Gauge::Hard { .. } => {} // Failed hard gauge can't be updated
        }

        //Clamp
        match self {
            Gauge::None => todo!(),
            Gauge::Normal { value, .. } => *value = value.clamp(0.0, 1.0),
            Gauge::Hard { value, .. } => *value = value.clamp(0.0, 1.0),
        }
    }

    pub fn is_cleared(&self) -> bool {
        match self {
            Gauge::Normal { value, .. } => *value >= 0.7,
            Gauge::Hard { value, .. } => *value >= 0.0,
            Gauge::None => false,
        }
    }

    pub fn is_dead(&self) -> bool {
        match self {
            Gauge::None => false,
            Gauge::Normal { .. } => false,
            Gauge::Hard { value, .. } => *value == 0.0,
        }
    }

    pub const fn value(&self) -> f32 {
        match self {
            Gauge::None => 0.0,
            Gauge::Normal { value, .. } => *value,
            Gauge::Hard { value, .. } => *value,
        }
    }

    pub fn update_sample(&mut self, sample: usize) {
        match self {
            Gauge::None => {}
            Gauge::Normal { value, samples, .. } => samples[sample.min(GAUGE_SAMPLES - 1)] = *value,
            Gauge::Hard { value, samples, .. } => samples[sample.min(GAUGE_SAMPLES - 1)] = *value,
        }
    }

    pub fn get_samples(&self) -> &[f32] {
        match self {
            Gauge::None => &[],
            Gauge::Normal { samples, .. } => samples.as_ref(),
            Gauge::Hard { samples, .. } => samples.as_ref(),
        }
    }
}

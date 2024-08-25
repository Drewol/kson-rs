#![allow(dead_code)]
use crate::{
    parameter::{BoolParameter, EffectParameter},
    Chart, Interval, Side, Track,
};

use kson_effect_param_macro::Effect;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use std::{borrow::Cow, collections::BTreeMap, f32, str::FromStr};

#[cfg(feature = "schema")]
use schemars::JsonSchema;

pub(crate) trait Effect {
    fn derive(&self, key: &str, param: &str) -> Self;
    fn param_list() -> &'static [&'static str];
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
#[serde(tag = "type", content = "v")]
#[serde(rename_all = "snake_case")]
pub enum AudioEffect {
    ReTrigger(ReTrigger),
    Gate(Gate),
    Flanger(Flanger),
    PitchShift(PitchShift),
    BitCrusher(BitCrusher),
    Phaser(Phaser),
    Wobble(Wobble),
    TapeStop(TapeStop),
    Echo(Echo),
    SideChain(SideChain),
    AudioSwap(String),
    HighPassFilter(HighPassFilter),
    LowPassFilter(LowPassFilter),
    PeakingFilter(PeakingFilter),
}

impl AudioEffect {
    pub fn name(&self) -> &'static str {
        match self {
            AudioEffect::ReTrigger(_) => "ReTrigger",
            AudioEffect::Gate(_) => "Gate",
            AudioEffect::Flanger(_) => "Flanger",
            AudioEffect::PitchShift(_) => "PitchShift",
            AudioEffect::BitCrusher(_) => "BitCrusher",
            AudioEffect::Phaser(_) => "Phaser",
            AudioEffect::Wobble(_) => "Wobble",
            AudioEffect::TapeStop(_) => "TapeStop",
            AudioEffect::Echo(_) => "Echo",
            AudioEffect::SideChain(_) => "SideChain",
            AudioEffect::AudioSwap(_) => "AudioSwap",
            AudioEffect::HighPassFilter(_) => "HighPassFilter",
            AudioEffect::LowPassFilter(_) => "LowPassFilter",
            AudioEffect::PeakingFilter(_) => "PeakingFilter",
        }
    }
}

impl TryFrom<&str> for AudioEffect {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "Retrigger" => Ok(AudioEffect::ReTrigger(ReTrigger::default())),
            "Gate" => Ok(AudioEffect::Gate(Gate::default())),
            "Flanger" => Ok(AudioEffect::Flanger(Flanger::default())),
            "PitchShift" => Ok(AudioEffect::PitchShift(PitchShift::default())),
            "BitCrusher" => Ok(AudioEffect::BitCrusher(BitCrusher::default())),
            "Phaser" => Ok(AudioEffect::Phaser(Phaser::default())),
            "Wobble" => Ok(AudioEffect::Wobble(Wobble::default())),
            "TapeStop" => Ok(AudioEffect::TapeStop(TapeStop::default())),
            "Echo" => Ok(AudioEffect::Echo(Echo::default())),
            "SideChain" => Ok(AudioEffect::SideChain(SideChain::default())),
            "SwitchAudio" => Ok(AudioEffect::AudioSwap("".to_owned())),
            "peak" => Ok(AudioEffect::PeakingFilter(PeakingFilter::default())),
            "hpf1" => Ok(AudioEffect::HighPassFilter(HighPassFilter::default())),
            "lpf1" => Ok(AudioEffect::LowPassFilter(LowPassFilter::default())),
            "bitc" => Ok(AudioEffect::BitCrusher(BitCrusher::default())),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Error)]
pub enum EffectError {
    #[error("Tried to apply effect changes with differing effect types.")]
    EffectTypeMismatchError,
}

impl Effect for String {
    fn derive(&self, _key: &str, param: &str) -> Self {
        param.to_string()
    }

    fn param_list() -> &'static [&'static str] {
        &[]
    }
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
pub struct ReTrigger {
    pub update_period: EffectParameter<f32>,
    pub wave_length: EffectParameter<f32>,
    pub rate: EffectParameter<f32>,
    pub update_trigger: BoolParameter,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
pub struct Gate {
    pub wave_length: EffectParameter<f32>,
    pub rate: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
pub struct Flanger {
    pub period: EffectParameter<f32>,
    pub delay: EffectParameter<i64>,
    pub depth: EffectParameter<i64>,
    pub feedback: EffectParameter<f32>,
    pub stereo_width: EffectParameter<f32>,
    pub vol: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
pub struct PitchShift {
    pub pitch: EffectParameter<f32>,
    pub pitch_quantize: BoolParameter,
    pub chunk_size: EffectParameter<i64>,
    pub overlap: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
pub struct BitCrusher {
    pub reduction: EffectParameter<i64>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
pub struct Phaser {
    pub period: EffectParameter<f32>,
    pub stage: EffectParameter<i64>,
    pub lo_freq: EffectParameter<f32>,
    pub hi_freq: EffectParameter<f32>,
    pub q: EffectParameter<f32>,
    pub feedback: EffectParameter<f32>,
    pub stereo_width: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
pub struct Wobble {
    pub wave_length: EffectParameter<f32>,
    pub lo_freq: EffectParameter<f32>,
    pub hi_freq: EffectParameter<f32>,
    pub q: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
pub struct TapeStop {
    pub speed: EffectParameter<f32>,
    pub trigger: BoolParameter,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
pub struct Echo {
    pub update_period: EffectParameter<f32>,
    pub wave_length: EffectParameter<f32>,
    pub update_trigger: BoolParameter,
    pub feedback_level: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
pub struct SideChain {
    pub period: EffectParameter<f32>,
    pub hold_time: EffectParameter<f32>,
    pub attack_time: EffectParameter<f32>,
    pub release_time: EffectParameter<f32>,
    pub ratio: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
pub struct HighPassFilter {
    pub v: EffectParameter<f32>,
    pub freq: EffectParameter<f32>,
    pub q: EffectParameter<f32>,
    pub delay: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
pub struct LowPassFilter {
    pub v: EffectParameter<f32>,
    pub freq: EffectParameter<f32>,
    pub q: EffectParameter<f32>,
    pub delay: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
pub struct PeakingFilter {
    pub v: EffectParameter<f32>,
    pub freq: EffectParameter<f32>,
    pub q: EffectParameter<f32>,
    pub gain: EffectParameter<f32>,
    pub delay: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Clone, Debug)]
pub struct EffectInterval {
    pub interval: Interval,
    pub effect: AudioEffect,
    pub track: Option<Track>,
    pub dom: bool,
}

fn default_param<T: Default>(val: &str) -> EffectParameter<T> {
    EffectParameter::from_str(val).unwrap_or_default()
}

impl Default for ReTrigger {
    fn default() -> Self {
        Self {
            update_period: default_param("1/2"),
            wave_length: default_param("1/4"),
            rate: default_param("70%"),
            update_trigger: default_param("off"),
            mix: default_param("0%>100%"),
        }
    }
}
impl Default for Gate {
    fn default() -> Self {
        Self {
            wave_length: default_param("1/4"),
            rate: default_param("70%"),
            mix: default_param("0%>90%"),
        }
    }
}
impl Default for Flanger {
    fn default() -> Self {
        Self {
            period: default_param("2.0"),
            delay: default_param("30samples"),
            depth: default_param("45samples"),
            feedback: default_param("60%"),
            stereo_width: default_param("0%"),
            vol: default_param("75%"),
            mix: default_param("0%>80%"),
        }
    }
}
impl Default for PitchShift {
    fn default() -> Self {
        Self {
            pitch: default_param("0"),
            pitch_quantize: default_param("0"),
            chunk_size: default_param("0"),
            overlap: default_param("0"),
            mix: default_param("0"),
        }
    }
}
impl Default for BitCrusher {
    fn default() -> Self {
        Self {
            reduction: default_param("0samples-30samples"),
            mix: default_param("0%>100%"),
        }
    }
}
impl Default for Phaser {
    fn default() -> Self {
        Self {
            period: default_param("0"),
            stage: default_param("0"),
            lo_freq: default_param("0"),
            hi_freq: default_param("0"),
            q: default_param("0"),
            feedback: default_param("0"),
            stereo_width: default_param("0"),
            mix: default_param("0"),
        }
    }
}
impl Default for Wobble {
    fn default() -> Self {
        Self {
            wave_length: default_param("1/12"),
            lo_freq: default_param("500Hz"),
            hi_freq: default_param("20000Hz"),
            q: default_param("1.414"),
            mix: default_param("0%>50%"),
        }
    }
}
impl Default for TapeStop {
    fn default() -> Self {
        Self {
            speed: default_param("50%"),
            trigger: default_param("off>on"),
            mix: default_param("0%>100%"),
        }
    }
}
impl Default for Echo {
    fn default() -> Self {
        Self {
            update_period: default_param("0"),
            wave_length: default_param("0"),
            update_trigger: default_param("0"),
            feedback_level: default_param("0"),
            mix: default_param("0"),
        }
    }
}
impl Default for SideChain {
    fn default() -> Self {
        Self {
            period: default_param("1/4"),
            hold_time: default_param("50ms"),
            attack_time: default_param("10ms"),
            release_time: default_param("1/16"),
            ratio: default_param("1>5"),
        }
    }
}
impl Default for HighPassFilter {
    fn default() -> Self {
        Self {
            v: default_param("0"),
            freq: default_param("80hz-2khz"),
            q: default_param("1.414"),
            delay: default_param("0"),
            mix: default_param("50%"),
        }
    }
}
impl Default for LowPassFilter {
    fn default() -> Self {
        Self {
            v: default_param("0"),
            freq: default_param("700hz-10khz"),
            q: default_param("1.414"),
            delay: default_param("0"),
            mix: default_param("50%"),
        }
    }
}
impl Default for PeakingFilter {
    fn default() -> Self {
        Self {
            v: default_param("0"),
            freq: default_param("80hz-8khz"),
            q: default_param("1.414"),
            gain: default_param("50%"),
            delay: default_param("0"),
            mix: default_param("50%"),
        }
    }
}

impl Chart {
    pub fn get_effect_tracks(&self) -> Vec<EffectInterval> {
        let audio_effect = &self.audio.audio_effect;
        let sides = [Side::Left, Side::Right];
        let mut result = vec![];
        for (name, root_effect) in &audio_effect.fx.def {
            if let Some(long_event) = audio_effect.fx.long_event.get(name) {
                for fx_side in 0..2 {
                    for event in &long_event[fx_side] {
                        if let Ok(note_index) =
                            self.note.fx[fx_side].binary_search_by_key(&event.0, |n| n.y)
                        {
                            let mut effect = audio_effect
                                .fx
                                .param_change
                                .get(name)
                                .map(|params_map| {
                                    params_map
                                        .iter()
                                        .flat_map(|(key, param_changes)| {
                                            param_changes
                                                .iter()
                                                .take_while(|(tick, _)| *tick <= event.0)
                                                .map(move |(tick, param)| (key, tick, param))
                                        })
                                        .fold(root_effect.clone(), |a, (key, _, param)| {
                                            a.derive(key, param)
                                        })
                                })
                                .unwrap_or_else(|| root_effect.clone());

                            if let Some(long_params) = &event.1 {
                                effect = long_params
                                    .iter()
                                    .fold(effect, |e, (key, param)| e.derive(key, param));
                            }
                            result.push(EffectInterval {
                                interval: self.note.fx[fx_side][note_index],
                                effect,
                                track: Some(Track::FX(sides[fx_side])),
                                dom: true,
                            });
                        }
                    }
                }
            }
        }

        for (i, side) in sides.iter().enumerate() {
            let intervals = self.note.laser[i].iter().map(|ls| Interval {
                y: ls.0,
                l: ls.1.last().map(|s| s.ry).unwrap_or(0),
            });

            for interval in intervals {
                if let Some((effect_key, Some(effect))) = audio_effect
                    .laser
                    .pulse_event
                    .iter()
                    .flat_map(|(a, b)| b.iter().map(move |(c, _)| (a, c)))
                    .take_while(|(_, tick)| **tick <= interval.y)
                    .max_by_key(|(_, tick)| **tick)
                    .map(|(k, _)| (k, audio_effect.laser.def.get(k)))
                {
                    let effect = audio_effect
                        .laser
                        .param_change
                        .get(effect_key)
                        .map(|a| {
                            a.iter()
                                .flat_map(|(param_name, changes)| {
                                    changes
                                        .iter()
                                        .map(move |(tick, param)| (*tick, param_name, param))
                                })
                                .take_while(|(tick, _, _)| *tick <= interval.y)
                                .fold(effect.clone(), |e, (_, key, param)| e.derive(key, param))
                        })
                        .unwrap_or_else(|| effect.clone());

                    result.push(EffectInterval {
                        interval,
                        effect,
                        track: Some(Track::Laser(*side)),
                        dom: true,
                    })
                }
            }

            //TODO: Mid-section effect changes
        }

        result.sort_by_key(|e| e.interval.y);
        result
    }

    pub fn laser_effect_queue(&self) -> std::collections::BTreeMap<u32, AudioEffect> {
        let laser = &self.audio.audio_effect.laser;

        let mut events = laser
            .pulse_event
            .iter()
            .flat_map(|(a, b)| {
                let a = Cow::from(a.clone());
                b.iter().copied().map(move |x| (a.clone(), x.0))
            })
            .collect::<Vec<_>>();

        events.sort_by_key(|x| x.1);

        let mut result = BTreeMap::new();

        for (key, y) in events.into_iter() {
            let Some(effect) = laser.def.get(key.as_ref()) else {
                continue;
            };
            let effect = laser
                .param_change
                .get(key.as_ref())
                .map(|a| {
                    a.iter()
                        .flat_map(|(param_name, changes)| {
                            changes
                                .iter()
                                .map(move |(tick, param)| (*tick, param_name, param))
                        })
                        .take_while(|(tick, _, _)| *tick <= y)
                        .fold(effect.clone(), |e, (_, key, param)| e.derive(key, param))
                })
                .unwrap_or_else(|| effect.clone());

            result.insert(y, effect);
        }

        result
    }
}

#![allow(dead_code)]
use crate::{
    parameter::{BoolParameter, EffectParameter, Parameter},
    Chart, Interval, Track,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use std::f32;

#[cfg(feature = "schema")]
use schemars::JsonSchema;

#[derive(Deserialize, Serialize, Clone)]
#[serde(tag = "type", content = "v")]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
#[derive(Debug, Error)]
pub enum EffectError {
    #[error("Tried to apply effect changes with differing effect types.")]
    EffectTypeMismatchError,
}

impl AudioEffect {
    fn apply_effect_event(&self, other: Self) -> Result<Self, EffectError> {
        //TODO: Proc macro maybe
        match (self, other) {
            (AudioEffect::ReTrigger(a), AudioEffect::ReTrigger(b)) => Ok({
                AudioEffect::ReTrigger(ReTrigger {
                    mix: a.mix.derive(&b.mix),
                    update_period: a.update_period.derive(&b.update_period),
                    update_period_tempo_sync: a
                        .update_period_tempo_sync
                        .derive(&b.update_period_tempo_sync),
                    wave_length: a.wave_length.derive(&b.wave_length),
                    wave_length_tempo_sync: a
                        .wave_length_tempo_sync
                        .derive(&b.wave_length_tempo_sync),
                    rate: a.rate.derive(&b.rate),
                    update_trigger: a.update_trigger.derive(&b.update_trigger),
                })
            }),
            (AudioEffect::Gate(_), AudioEffect::Gate(_)) => todo!(),
            (AudioEffect::Flanger(_), AudioEffect::Flanger(_)) => todo!(),
            (AudioEffect::PitchShift(_), AudioEffect::PitchShift(_)) => todo!(),
            (AudioEffect::BitCrusher(_), AudioEffect::BitCrusher(_)) => todo!(),
            (AudioEffect::Phaser(_), AudioEffect::Phaser(_)) => todo!(),
            (AudioEffect::Wobble(_), AudioEffect::Wobble(_)) => todo!(),
            (AudioEffect::TapeStop(_), AudioEffect::TapeStop(_)) => todo!(),
            (AudioEffect::Echo(_), AudioEffect::Echo(_)) => todo!(),
            (AudioEffect::SideChain(_), AudioEffect::SideChain(_)) => todo!(),
            (AudioEffect::AudioSwap(_), AudioEffect::AudioSwap(_)) => todo!(),
            (AudioEffect::HighPassFilter(_), AudioEffect::HighPassFilter(_)) => todo!(),
            (AudioEffect::LowPassFilter(_), AudioEffect::LowPassFilter(_)) => todo!(),
            (AudioEffect::PeakingFilter(_), AudioEffect::PeakingFilter(_)) => todo!(),
            _ => Err(EffectError::EffectTypeMismatchError),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ReTrigger {
    pub update_period: EffectParameter<f32>,
    pub update_period_tempo_sync: BoolParameter,
    pub wave_length: EffectParameter<f32>,
    pub wave_length_tempo_sync: BoolParameter,
    pub rate: EffectParameter<f32>,
    pub update_trigger: BoolParameter,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Gate {
    pub wave_length: EffectParameter<f32>,
    pub wave_length_tempo_sync: BoolParameter,
    pub rate: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Flanger {
    pub period: EffectParameter<f32>,
    pub period_tempo_sync: BoolParameter,
    pub delay: EffectParameter<i64>,
    pub depth: EffectParameter<i64>,
    pub feedback: EffectParameter<f32>,
    pub stereo_width: EffectParameter<f32>,
    pub vol: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PitchShift {
    pub pitch: EffectParameter<f32>,
    pub pitch_quantize: BoolParameter,
    pub chunk_size: EffectParameter<i64>,
    pub overlap: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BitCrusher {
    pub reduction: EffectParameter<i64>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Phaser {
    pub period: EffectParameter<f32>,
    pub period_tempo_sync: BoolParameter,
    pub stage: EffectParameter<i64>,
    pub lo_freq: EffectParameter<f32>,
    pub hi_freq: EffectParameter<f32>,
    pub q: EffectParameter<f32>,
    pub feedback: EffectParameter<f32>,
    pub stereo_width: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Wobble {
    pub wave_length: EffectParameter<f32>,
    pub wave_length_tempo_sync: BoolParameter,
    pub lo_freq: EffectParameter<f32>,
    pub hi_freq: EffectParameter<f32>,
    pub q: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TapeStop {
    pub speed: EffectParameter<f32>,
    pub trigger: BoolParameter,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Echo {
    pub update_period: EffectParameter<f32>,
    pub update_period_tempo_sync: BoolParameter,
    pub wave_length: EffectParameter<f32>,
    pub wave_length_tempo_sync: BoolParameter,
    pub update_trigger: BoolParameter,
    pub feedback_level: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SideChain {
    pub period: EffectParameter<f32>,
    pub period_tempo_sync: BoolParameter,
    pub hold_time: EffectParameter<f32>,
    pub hold_time_tempo_sync: BoolParameter,
    pub attack_time: EffectParameter<f32>,
    pub attack_time_tempo_sync: BoolParameter,
    pub release_time: EffectParameter<f32>,
    pub release_time_tempo_sync: BoolParameter,
    pub ratio: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct HighPassFilter {
    pub env: EffectParameter<f32>,
    pub lo_freq: EffectParameter<f32>,
    pub hi_freq: EffectParameter<f32>,
    pub q: EffectParameter<f32>,
    pub delay: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct LowPassFilter {
    pub env: EffectParameter<f32>,
    pub lo_freq: EffectParameter<f32>,
    pub hi_freq: EffectParameter<f32>,
    pub q: EffectParameter<f32>,
    pub delay: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PeakingFilter {
    pub env: EffectParameter<f32>,
    pub lo_freq: EffectParameter<f32>,
    pub hi_freq: EffectParameter<f32>,
    pub q: EffectParameter<f32>,
    pub delay: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

pub struct EffectInterval {
    interval: Interval,
    effect: AudioEffect,
    track: Option<Track>,
    dom: bool,
}

impl Chart {
    pub fn get_effect_tracks(&self) -> Vec<EffectInterval> {
        if self.audio.audio_effect.is_none() {
            return vec![];
        }

        let audio_effect_info = self.audio.audio_effect.as_ref().unwrap();

        let (effect_def, effect_note_event) =
            match (&audio_effect_info.def, &audio_effect_info.note_event) {
                (Some(def), Some(event)) => (def, event),
                _ => return vec![],
            };

        let mut effect_intervals = vec![];

        for (effect_name, effect_events) in effect_note_event {
            if !effect_def.contains_key(effect_name) {
                continue;
            }

            let original_effect = effect_def.get(effect_name).unwrap();

            for (effect_event, track) in effect_events {
                let new_effect = original_effect.clone();
                let interval = match track {
                    Track::BT(l) => self.note.bt[l as usize]
                        .iter()
                        .find(|b| b.y == effect_event.y)
                        .cloned(),
                    Track::FX(l) => self.note.fx[l as usize]
                        .iter()
                        .find(|b| b.y == effect_event.y)
                        .cloned(),
                    Track::Laser(l) => self.note.laser[l as usize]
                        .iter()
                        .find(|s| s.y == effect_event.y)
                        .map(|s| Interval {
                            y: s.y,
                            l: s.v.last().map(|p| p.ry).unwrap_or(0),
                        }),
                };

                //TODO: Apply effect_event.v on top of new_effect instead of this

                if let Some(interval) = interval {
                    effect_intervals.push(EffectInterval {
                        interval,
                        effect: effect_event.v.as_ref().cloned().unwrap_or(new_effect),
                        track: Some(track),
                        dom: effect_event.dom,
                    })
                }
            }
        }

        effect_intervals
    }
}

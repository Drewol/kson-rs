#![allow(dead_code)]
use crate::parameter::{BoolParameter, EffectParameter};
use serde::{Deserialize, Serialize};

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

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ReTrigger {
    update_period: EffectParameter<f32>,
    update_period_tempo_sync: BoolParameter,
    wave_length: EffectParameter<f32>,
    wave_length_tempo_sync: BoolParameter,
    rate: EffectParameter<f32>,
    update_trigger: BoolParameter,
    mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Gate {
    wave_length: EffectParameter<f32>,
    wave_length_tempo_sync: BoolParameter,
    rate: EffectParameter<f32>,
    mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Flanger {
    period: EffectParameter<f32>,
    period_tempo_sync: BoolParameter,
    delay: EffectParameter<i64>,
    depth: EffectParameter<i64>,
    feedback: EffectParameter<f32>,
    stereo_width: EffectParameter<f32>,
    vol: EffectParameter<f32>,
    mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PitchShift {
    pitch: EffectParameter<f32>,
    pitch_quantize: BoolParameter,
    chunk_size: EffectParameter<i64>,
    overlap: EffectParameter<f32>,
    mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BitCrusher {
    reduction: EffectParameter<i64>,
    mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Phaser {
    period: EffectParameter<f32>,
    period_tempo_sync: BoolParameter,
    stage: EffectParameter<i64>,
    lo_freq: EffectParameter<f32>,
    hi_freq: EffectParameter<f32>,
    q: EffectParameter<f32>,
    feedback: EffectParameter<f32>,
    stereo_width: EffectParameter<f32>,
    mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Wobble {
    wave_length: EffectParameter<f32>,
    wave_length_tempo_sync: BoolParameter,
    lo_freq: EffectParameter<f32>,
    hi_freq: EffectParameter<f32>,
    q: EffectParameter<f32>,
    mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TapeStop {
    speed: EffectParameter<f32>,
    trigger: BoolParameter,
    mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Echo {
    update_period: EffectParameter<f32>,
    update_period_tempo_sync: BoolParameter,
    wave_length: EffectParameter<f32>,
    wave_length_tempo_sync: BoolParameter,
    update_trigger: BoolParameter,
    feedback_level: EffectParameter<f32>,
    mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SideChain {
    period: EffectParameter<f32>,
    period_tempo_sync: BoolParameter,
    hold_time: EffectParameter<f32>,
    hold_time_tempo_sync: BoolParameter,
    attack_time: EffectParameter<f32>,
    attack_time_tempo_sync: BoolParameter,
    release_time: EffectParameter<f32>,
    release_time_tempo_sync: BoolParameter,
    ratio: EffectParameter<f32>,
}

#[derive(Copy, Clone)]
pub enum BiQuadType {
    Peaking(f32),
    LowPass,
    HighPass,
}

impl Default for BiQuadType {
    fn default() -> BiQuadType {
        let two: f32 = 2.0;
        BiQuadType::Peaking(two.sqrt())
    }
}

#[derive(Default, Clone)]
pub struct BiQuad {
    a0: f32,
    a1: f32,
    a2: f32,
    b0: f32,
    b1: f32,
    b2: f32,
    za: Vec<[f32; 2]>,
    zb: Vec<[f32; 2]>,
    q: f32,
    rate: u32,
    mix: f32,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct HighPassFilter {
    env: EffectParameter<f32>,
    lo_freq: EffectParameter<f32>,
    hi_freq: EffectParameter<f32>,
    q: EffectParameter<f32>,
    delay: EffectParameter<f32>,
    mix: EffectParameter<f32>,
    #[serde(skip_deserializing, skip_serializing)]
    filter: BiQuad,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct LowPassFilter {
    env: EffectParameter<f32>,
    lo_freq: EffectParameter<f32>,
    hi_freq: EffectParameter<f32>,
    q: EffectParameter<f32>,
    delay: EffectParameter<f32>,
    mix: EffectParameter<f32>,
    #[serde(skip_deserializing, skip_serializing)]
    filter: BiQuad,
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PeakingFilter {
    env: EffectParameter<f32>,
    lo_freq: EffectParameter<f32>,
    hi_freq: EffectParameter<f32>,
    q: EffectParameter<f32>,
    delay: EffectParameter<f32>,
    mix: EffectParameter<f32>,
    #[serde(skip_deserializing, skip_serializing)]
    filter: BiQuad,
}

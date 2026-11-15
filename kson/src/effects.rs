#![allow(dead_code)]
use crate::{
    parameter::{BoolParameter, EffectParameter, ParamType},
    Chart, Interval, Side, Track,
};

use kson_effect_param_macro::Effect;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use std::{borrow::Cow, collections::BTreeMap, f32};

pub(crate) trait Effect {
    fn derive(&self, key: &str, param: &str) -> Self;
    fn param_list() -> &'static [&'static str];
}

#[derive(Serialize, Clone, Effect, PartialEq, Debug)]
#[serde(tag = "type", content = "v")]
#[serde(rename_all = "snake_case")]
pub enum AudioEffect {
    Retrigger(ReTrigger),
    Gate(Gate),
    Flanger(Flanger),
    PitchShift(PitchShift),
    #[serde(rename = "bitcrusher")]
    BitCrusher(BitCrusher),
    Phaser(Phaser),
    Wobble(Wobble),
    Tapestop(TapeStop),
    Echo(Echo),
    Sidechain(Sidechain),
    SwitchAudio(SwitchAudio),
    HighPassFilter(HighPassFilter),
    LowPassFilter(LowPassFilter),
    PeakingFilter(PeakingFilter),
}

impl<'de> Deserialize<'de> for AudioEffect {
    /// `{"type": "...", "v": {...}}` where `v` is optional and only overrides the
    /// predefined default parameter values
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawEffect {
            #[serde(rename = "type")]
            effect_type: String,
            #[serde(default)]
            v: std::collections::HashMap<String, String>,
        }

        let raw = RawEffect::deserialize(deserializer)?;
        let effect_type = match raw.effect_type.as_str() {
            "bit_crusher" => "bitcrusher",
            "tape_stop" => "tapestop",
            v => v,
        };
        let effect = AudioEffect::builtin(effect_type).ok_or_else(|| {
            serde::de::Error::custom(format!("unknown audio effect type `{}`", raw.effect_type))
        })?;

        Ok(raw
            .v
            .iter()
            .fold(effect, |e, (key, value)| e.derive(key, value)))
    }
}

impl AudioEffect {
    pub fn name(&self) -> &'static str {
        match self {
            AudioEffect::Retrigger(_) => "ReTrigger",
            AudioEffect::Gate(_) => "Gate",
            AudioEffect::Flanger(_) => "Flanger",
            AudioEffect::PitchShift(_) => "PitchShift",
            AudioEffect::BitCrusher(_) => "BitCrusher",
            AudioEffect::Phaser(_) => "Phaser",
            AudioEffect::Wobble(_) => "Wobble",
            AudioEffect::Tapestop(_) => "Tapestop",
            AudioEffect::Echo(_) => "Echo",
            AudioEffect::Sidechain(_) => "SideChain",
            AudioEffect::SwitchAudio(_) => "SwitchAudio",
            AudioEffect::HighPassFilter(_) => "HighPassFilter",
            AudioEffect::LowPassFilter(_) => "LowPassFilter",
            AudioEffect::PeakingFilter(_) => "PeakingFilter",
        }
    }

    /// The `mix` parameter (blending ratio of the original and effect audio), if any
    pub fn mix(&self) -> Option<&EffectParameter<f32>> {
        Some(match self {
            AudioEffect::Retrigger(e) => &e.mix,
            AudioEffect::Gate(e) => &e.mix,
            AudioEffect::Flanger(e) => &e.mix,
            AudioEffect::PitchShift(e) => &e.mix,
            AudioEffect::BitCrusher(e) => &e.mix,
            AudioEffect::Phaser(e) => &e.mix,
            AudioEffect::Wobble(e) => &e.mix,
            AudioEffect::Tapestop(e) => &e.mix,
            AudioEffect::Echo(e) => &e.mix,
            AudioEffect::HighPassFilter(e) => &e.mix,
            AudioEffect::LowPassFilter(e) => &e.mix,
            AudioEffect::PeakingFilter(e) => &e.mix,
            AudioEffect::Sidechain(_) | AudioEffect::SwitchAudio(_) => return None,
        })
    }
}

impl TryFrom<&str> for AudioEffect {
    type Error = ();

    /// Accepts KSH effect names (both "`ReTrigger`" and the spec spelling "`Retrigger`"),
    /// KSH laser filter names ("`peak`", "`hpf1`", ...), and kson effect type names.
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if let Some(e) = AudioEffect::builtin(value) {
            return Ok(e);
        }
        AudioEffect::ksh_name_to_kson(value)
            .and_then(AudioEffect::builtin)
            .ok_or(())
    }
}

impl AudioEffect {
    /// Maps a KSH effect name (`fx-l`/`fx-r`/`filtertype`/`#define_xx type=`) to the kson effect type name
    pub fn ksh_name_to_kson(value: &str) -> Option<&'static str> {
        Some(match value {
            "Retrigger" | "ReTrigger" => "retrigger",
            "Gate" => "gate",
            "Flanger" => "flanger",
            "PitchShift" => "pitch_shift",
            "BitCrusher" | "bitc" => "bitcrusher",
            "Phaser" => "phaser",
            "Wobble" => "wobble",
            "TapeStop" | "Tapestop" => "tapestop",
            "Echo" => "echo",
            "SideChain" => "sidechain",
            "SwitchAudio" => "switch_audio",
            "PeakingFilter" | "peak" => "peaking_filter",
            "HighPassFilter" | "hpf1" => "high_pass_filter",
            "LowPassFilter" | "lpf1" => "low_pass_filter",
            _ => return None,
        })
    }

    /// Returns the predefined effect with default parameters for a kson effect type name
    pub fn builtin(kson_name: &str) -> Option<AudioEffect> {
        Some(match kson_name {
            "retrigger" => AudioEffect::Retrigger(ReTrigger::default()),
            "gate" => AudioEffect::Gate(Gate::default()),
            "flanger" => AudioEffect::Flanger(Flanger::default()),
            "pitch_shift" => AudioEffect::PitchShift(PitchShift::default()),
            "bitcrusher" => AudioEffect::BitCrusher(BitCrusher::default()),
            "phaser" => AudioEffect::Phaser(Phaser::default()),
            "wobble" => AudioEffect::Wobble(Wobble::default()),
            "tapestop" => AudioEffect::Tapestop(TapeStop::default()),
            "echo" => AudioEffect::Echo(Echo::default()),
            "sidechain" => AudioEffect::Sidechain(Sidechain::default()),
            "switch_audio" => AudioEffect::SwitchAudio(SwitchAudio::default()),
            "peaking_filter" => AudioEffect::PeakingFilter(PeakingFilter::default()),
            "high_pass_filter" => AudioEffect::HighPassFilter(HighPassFilter::default()),
            "low_pass_filter" => AudioEffect::LowPassFilter(LowPassFilter::default()),
            _ => return None,
        })
    }

    /// Resolves an effect name: user definitions override predefined effects.
    /// Returns `None` for unknown names and for the predefined "no effect" name `""`.
    pub fn resolve<'a>(
        defs: &'a [crate::DefKeyValuePair<AudioEffect>],
        name: &str,
    ) -> Option<Cow<'a, AudioEffect>> {
        if name.is_empty() {
            return None;
        }
        defs.iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| Cow::Borrowed(v))
            .or_else(|| AudioEffect::builtin(name).map(Cow::Owned))
    }
}

#[derive(Debug, Error)]
pub enum EffectError {
    #[error("Tried to apply effect changes with differing effect types.")]
    EffectTypeMismatchError,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct SwitchAudio {
    pub filename: String,
}

impl Effect for SwitchAudio {
    fn derive(&self, _key: &str, param: &str) -> Self {
        SwitchAudio {
            filename: param.to_string(),
        }
    }

    fn param_list() -> &'static [&'static str] {
        &["filename"]
    }
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
#[serde(default)]
pub struct ReTrigger {
    pub update_period: EffectParameter<f32>,
    pub wave_length: EffectParameter<f32>,
    pub rate: EffectParameter<f32>,
    pub update_trigger: BoolParameter,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
#[serde(default)]
pub struct Gate {
    pub wave_length: EffectParameter<f32>,
    pub rate: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
#[serde(default)]
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
#[serde(default)]
pub struct PitchShift {
    pub pitch: EffectParameter<f32>,
    pub chunk_size: EffectParameter<i64>,
    pub overlap: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
#[serde(default)]
pub struct BitCrusher {
    pub reduction: EffectParameter<i64>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
#[serde(default)]
pub struct Phaser {
    pub period: EffectParameter<f32>,
    pub stage: EffectParameter<i64>,
    #[serde(alias = "lo_freq")]
    pub freq_1: EffectParameter<f32>,
    #[serde(alias = "hi_freq")]
    pub freq_2: EffectParameter<f32>,
    pub q: EffectParameter<f32>,
    pub feedback: EffectParameter<f32>,
    pub stereo_width: EffectParameter<f32>,
    pub hi_cut_gain: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
#[serde(default)]
pub struct Wobble {
    pub wave_length: EffectParameter<f32>,
    #[serde(alias = "lo_freq")]
    pub freq_1: EffectParameter<f32>,
    #[serde(alias = "hi_freq")]
    pub freq_2: EffectParameter<f32>,
    pub q: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
#[serde(default)]
pub struct TapeStop {
    pub speed: EffectParameter<f32>,
    pub trigger: BoolParameter,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
#[serde(default)]
pub struct Echo {
    pub update_period: EffectParameter<f32>,
    pub wave_length: EffectParameter<f32>,
    pub update_trigger: BoolParameter,
    pub feedback_level: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
#[serde(default)]
pub struct Sidechain {
    pub period: EffectParameter<f32>,
    pub hold_time: EffectParameter<f32>,
    pub attack_time: EffectParameter<f32>,
    /// `None` when not specified; see [`Sidechain::release_time_or_default`]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_time: Option<EffectParameter<f32>>,
    pub ratio: EffectParameter<f32>,
}

impl Sidechain {
    /// `release_time`, or its default when not specified: `1/8`, or `1/16` for user-defined
    /// effects when `compat.ksh_version` is earlier than "200" (spec: `sidechain.release_time`)
    pub fn release_time_or_default(&self, legacy_user_defined: bool) -> EffectParameter<f32> {
        self.release_time.clone().unwrap_or_else(|| {
            default_param(
                "release_time",
                if legacy_user_defined { "1/16" } else { "1/8" },
            )
        })
    }
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
#[serde(default)]
pub struct HighPassFilter {
    pub v: EffectParameter<f32>,
    /// Cutoff frequency when `v` is 0.0
    pub freq: EffectParameter<f32>,
    /// Cutoff frequency when `v` is 1.0
    pub freq_max: EffectParameter<f32>,
    pub q: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
#[serde(default)]
pub struct LowPassFilter {
    pub v: EffectParameter<f32>,
    /// Cutoff frequency when `v` is 0.0
    pub freq: EffectParameter<f32>,
    /// Cutoff frequency when `v` is 1.0
    pub freq_max: EffectParameter<f32>,
    pub q: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Deserialize, Serialize, Clone, Effect, PartialEq, Debug)]
#[serde(default)]
pub struct PeakingFilter {
    pub v: EffectParameter<f32>,
    /// Center frequency when `v` is 0.0
    pub freq: EffectParameter<f32>,
    /// Center frequency when `v` is 1.0
    pub freq_max: EffectParameter<f32>,
    /// Bandwidth around the center frequency in octaves
    pub bandwidth: EffectParameter<f32>,
    pub gain: EffectParameter<f32>,
    pub mix: EffectParameter<f32>,
}

#[derive(Clone, Debug)]
pub struct EffectInterval {
    pub interval: Interval,
    pub effect: AudioEffect,
    pub track: Option<Track>,
    pub dom: bool,
}

/// Parses a predefined default value as the spec type of the parameter `name`
fn default_param<T: Default>(name: &str, val: &str) -> EffectParameter<T> {
    let param = EffectParameter::parse_typed(val, ParamType::for_name(name));
    debug_assert!(param.is_ok(), "invalid default {name}={val}");
    param.unwrap_or_default()
}

// Defaults are from the spec ("Audio effects & parameter list"). The filter `freq`,
// `freq_max`, `q` and `bandwidth` defaults are implementation-dependent: they keep the
// previous kson-rs ranges, with the low-pass filter fully open at `v` = 0.
impl Default for ReTrigger {
    fn default() -> Self {
        Self {
            update_period: default_param("update_period", "1/2"),
            wave_length: default_param("wave_length", "0"),
            rate: default_param("rate", "70%"),
            update_trigger: default_param("update_trigger", "off"),
            mix: default_param("mix", "0%>100%"),
        }
    }
}
impl Default for Gate {
    fn default() -> Self {
        Self {
            wave_length: default_param("wave_length", "0"),
            rate: default_param("rate", "50%"),
            mix: default_param("mix", "0%>90%"),
        }
    }
}
impl Default for Flanger {
    fn default() -> Self {
        Self {
            period: default_param("period", "2.0"),
            delay: default_param("delay", "30samples"),
            depth: default_param("depth", "45samples"),
            feedback: default_param("feedback", "60%"),
            stereo_width: default_param("stereo_width", "0%"),
            vol: default_param("vol", "75%"),
            mix: default_param("mix", "0%>80%"),
        }
    }
}
impl Default for PitchShift {
    fn default() -> Self {
        Self {
            pitch: default_param("pitch", "0>12"),
            chunk_size: default_param("chunk_size", "700samples"),
            overlap: default_param("overlap", "40%"),
            mix: default_param("mix", "0%>100%"),
        }
    }
}
impl Default for BitCrusher {
    fn default() -> Self {
        Self {
            reduction: default_param("reduction", "0samples-30samples"),
            mix: default_param("mix", "0%>100%"),
        }
    }
}
impl Default for Phaser {
    fn default() -> Self {
        Self {
            period: default_param("period", "1/2"),
            stage: default_param("stage", "6"),
            freq_1: default_param("freq_1", "1500Hz"),
            freq_2: default_param("freq_2", "20000Hz"),
            q: default_param("q", "0.707"),
            feedback: default_param("feedback", "35%"),
            stereo_width: default_param("stereo_width", "75%"),
            hi_cut_gain: default_param("hi_cut_gain", "-8.0dB"),
            mix: default_param("mix", "0%>50%"),
        }
    }
}
impl Default for Wobble {
    fn default() -> Self {
        Self {
            wave_length: default_param("wave_length", "0"),
            freq_1: default_param("freq_1", "500Hz"),
            freq_2: default_param("freq_2", "20000Hz"),
            q: default_param("q", "1.414"),
            mix: default_param("mix", "0%>80%"),
        }
    }
}
impl Default for TapeStop {
    fn default() -> Self {
        Self {
            speed: default_param("speed", "50%"),
            trigger: default_param("trigger", "off>on"),
            mix: default_param("mix", "0%>100%"),
        }
    }
}
impl Default for Echo {
    fn default() -> Self {
        Self {
            update_period: default_param("update_period", "0"),
            wave_length: default_param("wave_length", "0"),
            update_trigger: default_param("update_trigger", "off>on"),
            feedback_level: default_param("feedback_level", "60%"),
            mix: default_param("mix", "0%>100%"),
        }
    }
}
impl Default for Sidechain {
    fn default() -> Self {
        Self {
            period: default_param("period", "1/4"),
            hold_time: default_param("hold_time", "50ms"),
            attack_time: default_param("attack_time", "10ms"),
            release_time: None,
            ratio: default_param("ratio", "1.0>5.0"),
        }
    }
}
impl Default for HighPassFilter {
    fn default() -> Self {
        Self {
            v: default_param("v", "0%-100%"),
            freq: default_param("freq", "80Hz"),
            freq_max: default_param("freq_max", "2kHz"),
            q: default_param("q", "1.414"),
            mix: default_param("mix", "0%>100%"),
        }
    }
}
impl Default for LowPassFilter {
    fn default() -> Self {
        Self {
            v: default_param("v", "0%-100%"),
            freq: default_param("freq", "10kHz"),
            freq_max: default_param("freq_max", "700Hz"),
            q: default_param("q", "1.414"),
            mix: default_param("mix", "0%>100%"),
        }
    }
}
impl Default for PeakingFilter {
    fn default() -> Self {
        Self {
            v: default_param("v", "0%-100%"),
            freq: default_param("freq", "80Hz"),
            freq_max: default_param("freq_max", "8kHz"),
            bandwidth: default_param("bandwidth", "1.0"),
            gain: default_param("gain", "50%"),
            mix: default_param("mix", "0%>100%"),
        }
    }
}

/// Applies all `param_change` entries for `name` at or before `y`
fn apply_param_changes(
    effect: AudioEffect,
    param_change: &crate::Dict<crate::Dict<crate::ByPulse<String>>>,
    name: &str,
    y: u32,
) -> AudioEffect {
    let Some(params) = param_change.get(name) else {
        return effect;
    };
    params.iter().fold(effect, |e, (key, changes)| {
        match changes.iter().rev().find(|(tick, _)| *tick <= y) {
            Some((_, param)) => e.derive(key, param),
            None => e,
        }
    })
}

impl Chart {
    /// Resolves an effect name like [`AudioEffect::resolve`] and fills in defaults that
    /// depend on the chart (the legacy sidechain `release_time`)
    fn resolve_effect(
        &self,
        defs: &[crate::DefKeyValuePair<AudioEffect>],
        name: &str,
    ) -> Option<AudioEffect> {
        let effect = AudioEffect::resolve(defs, name)?;
        let user_defined = matches!(effect, Cow::Borrowed(_));
        let mut effect = effect.into_owned();
        if let AudioEffect::Sidechain(s) = &mut effect {
            let legacy = user_defined && self.compat.as_ref().is_some_and(|c| c.is_legacy_ksh());
            s.release_time = Some(s.release_time_or_default(legacy));
        }
        Some(effect)
    }

    pub fn get_effect_tracks(&self) -> Vec<EffectInterval> {
        let audio_effect = &self.audio.audio_effect;
        let sides = [Side::Left, Side::Right];
        let mut result = vec![];

        // FX: events may be placed anywhere in [y, y + l) of a long note; later events
        // replace the effect for the rest of the note.
        for (fx_side, side) in sides.iter().enumerate() {
            let mut events = audio_effect
                .fx
                .long_event
                .iter()
                .flat_map(|(name, lanes)| lanes[fx_side].iter().map(move |e| (name, e)))
                .collect::<Vec<_>>();
            events.sort_by_key(|(_, e)| e.0);

            for note in self.note.fx[fx_side].iter().filter(|n| n.l > 0) {
                let note_events = events
                    .iter()
                    .filter(|(_, e)| e.0 >= note.y && e.0 < note.y + note.l)
                    .collect::<Vec<_>>();

                for (i, (name, event)) in note_events.iter().enumerate() {
                    let start = event.0;
                    let end = note_events
                        .get(i + 1)
                        .map(|(_, e)| e.0)
                        .unwrap_or(note.y + note.l);
                    if end <= start {
                        continue;
                    }
                    let Some(root_effect) = self.resolve_effect(&audio_effect.fx.def, name) else {
                        continue;
                    };
                    let mut effect = apply_param_changes(
                        root_effect,
                        &audio_effect.fx.param_change,
                        name,
                        start,
                    );
                    if let Some(long_params) = &event.1 {
                        effect = long_params
                            .iter()
                            .fold(effect, |e, (key, param)| e.derive(key, param));
                    }
                    result.push(EffectInterval {
                        interval: Interval {
                            y: start,
                            l: end - start,
                        },
                        effect,
                        track: Some(Track::FX(*side)),
                        dom: true,
                    });
                }
            }
        }

        // Laser: the latest pulse event at or before a point decides the effect
        let mut laser_events = audio_effect
            .laser
            .pulse_event
            .iter()
            .flat_map(|(name, ticks)| ticks.iter().map(move |y| (*y, name)))
            .collect::<Vec<_>>();
        laser_events.sort_by_key(|(y, _)| *y);

        for (i, side) in sides.iter().enumerate() {
            for section in &self.note.laser[i] {
                let section_start = section.0;
                let section_end = section_start + section.1.last().map(|s| s.ry).unwrap_or(0);

                let mut boundaries = vec![];
                if let Some(active) = laser_events.iter().rev().find(|(y, _)| *y <= section_start) {
                    boundaries.push((section_start, active.1));
                }
                boundaries.extend(
                    laser_events
                        .iter()
                        .filter(|(y, _)| *y > section_start && *y < section_end)
                        .map(|(y, name)| (*y, *name)),
                );

                for (bi, (start, name)) in boundaries.iter().enumerate() {
                    let end = boundaries.get(bi + 1).map(|b| b.0).unwrap_or(section_end);
                    let Some(effect) = self.resolve_effect(&audio_effect.laser.def, name) else {
                        continue;
                    };
                    let effect = apply_param_changes(
                        effect,
                        &audio_effect.laser.param_change,
                        name,
                        *start,
                    );

                    result.push(EffectInterval {
                        interval: Interval {
                            y: *start,
                            l: end.saturating_sub(*start),
                        },
                        effect,
                        track: Some(Track::Laser(*side)),
                        dom: true,
                    })
                }
            }
        }

        result.sort_by_key(|e| e.interval.y);
        result
    }

    pub fn laser_effect_queue(&self) -> std::collections::BTreeMap<u32, AudioEffect> {
        let laser = &self.audio.audio_effect.laser;

        let mut result = BTreeMap::new();

        for (key, ticks) in &laser.pulse_event {
            let Some(effect) = self.resolve_effect(&laser.def, key) else {
                continue;
            };
            for y in ticks {
                result.insert(
                    *y,
                    apply_param_changes(effect.clone(), &laser.param_change, key, *y),
                );
            }
        }

        result
    }
}

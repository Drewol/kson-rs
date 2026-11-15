use anyhow::Result;
use itertools::Itertools;
use kson::overlaps::Overlaps;
use kson::Chart;

use rodio::source::{Buffered, SkipDuration};
pub use rodio::Source;
use rodio::{ChannelCount, SampleRate};

use std::collections::VecDeque;
use std::fs::File;
use std::io::BufReader;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use kson_rodio_sources::{
    self,
    bitcrush::bit_crusher,
    effected_part::effected_part,
    flanger::flanger,
    gate::gate,
    biquad::biquad,
    mix_source::{MixSource, NoMix},
    phaser::{phaser, PhaserParams},
    pitch_shift::pitch_shift,
    re_trigger::re_trigger,
    side_chain::side_chain,
    tape_stop::tape_stop,
    wobble::wobble,
};

type ActiveEffect = ((u64, u64), Box<dyn Source + Send>);

pub struct AudioFile {
    audio: SkipDuration<Buffered<Box<dyn Source + Send>>>,
    audio_base: SkipDuration<Buffered<Box<dyn Source + Send>>>,
    effected: Option<SkipDuration<Buffered<Box<dyn Source + Send>>>>,
    effected_base: Option<SkipDuration<Buffered<Box<dyn Source + Send>>>>,
    leadin: Arc<AtomicUsize>,
    stopped: Arc<AtomicBool>,
    fx_enable: [Arc<AtomicBool>; 2],
    channels: ChannelCount,
    sample_rate: SampleRate,
    pos: Arc<AtomicUsize>,
    effects: VecDeque<((u64, u64), Box<EffectBuilder>)>,
    active_effects: Vec<ActiveEffect>,
}

pub struct EventList<T> {
    events: Vec<(u32, T)>,
}

impl<T> EventList<T> {
    pub fn update(&mut self, tick: &u32, f: &dyn Fn(&T)) {
        //while let in case of multiple events on a tick
        //or if multiple ticks passed since the last update.
        while let Some((event_tick, value)) = self.events.first() {
            if event_tick <= tick {
                f(value);
                self.events.remove(0);
            } else {
                return;
            }
        }
    }

    pub fn add(&mut self, tick: u32, value: T) {
        match self.events.binary_search_by(|t| t.0.cmp(&tick)) {
            Ok(index) => self.events.insert(index, (tick, value)),
            Err(index) => self.events.insert(index, (tick, value)),
        }
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }
}

impl Iterator for AudioFile {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.stopped.load(Ordering::Relaxed) {
            self.pos.store(0, Ordering::Relaxed);
            return None;
        }

        let leadin = self.leadin.load(Ordering::Relaxed);
        if leadin > 0 {
            self.leadin.store(leadin - 1, Ordering::Relaxed);
            return Some(0.0);
        }

        let enable_fx = self.fx_enable.iter().any(|x| x.load(Ordering::Relaxed));

        let pos = self.pos.fetch_add(1, Ordering::Relaxed);
        let base = self.audio.next();
        let effected = self
            .active_effects
            .iter_mut()
            .map(|x| x.1.next())
            .last()
            .flatten();

        self.active_effects
            .retain(|((_, end), _)| *end > (pos as u64));

        while let Some(((start, end), builder)) = self.effects.pop_front() {
            if start > pos as _ {
                self.effects.push_front(((start, end), builder));
                break;
            }

            let new_effect = builder(Box::new(self.audio.clone()));

            self.active_effects.push(((start, end), new_effect));
        }

        if effected.is_some() && enable_fx {
            effected
        } else {
            base
        }
    }
}

impl Source for AudioFile {
    fn current_span_len(&self) -> Option<usize> {
        self.audio.current_span_len()
    }

    #[inline]
    fn channels(&self) -> ChannelCount {
        self.channels
    }

    #[inline]
    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    #[inline]
    fn total_duration(&self) -> Option<std::time::Duration> {
        self.audio.total_duration()
    }
}

impl AudioFile {
    fn get_ms(&self) -> f64 {
        let leadin = self.leadin.load(Ordering::Relaxed);

        if leadin > 0 {
            -((leadin / self.channels.get() as usize) as f64
                / (self.sample_rate.get() as f64 / 1000.0))
        } else {
            (self.pos.load(Ordering::SeqCst) / self.channels.get() as usize) as f64
                / (self.sample_rate.get() as f64 / 1000.0)
        }
    }

    fn set_stopped(&mut self, val: bool) {
        self.stopped.store(val, Ordering::SeqCst);
    }

    fn set_leadin(&self, duration: Duration) {
        self.leadin.store(
            ((duration.as_millis() * self.sample_rate.get() as u128) / 1000) as usize
                * self.channels.get() as usize,
            Ordering::Relaxed,
        )
    }
}

type EffectBuilder =
    dyn Fn(Box<dyn Source<Item = f32> + Send>) -> Box<dyn Source<Item = f32> + Send> + Send;

pub struct AudioPlayback {
    file: Option<AudioFile>,
    last_file: String,
    effects: Vec<((u64, u64), Box<EffectBuilder>)>,
    leadin: Duration,
}

impl AudioPlayback {
    pub fn new() -> Self {
        AudioPlayback {
            file: None,
            last_file: String::new(),
            effects: vec![],
            leadin: Duration::ZERO,
        }
    }

    pub fn set_fx_enable(&mut self, left: bool, right: bool) {
        if let Some(file) = &self.file {
            file.fx_enable[0].store(left, Ordering::Relaxed);
            file.fx_enable[1].store(right, Ordering::Relaxed);
        }
    }

    pub fn set_leadin(&mut self, duration: Duration) {
        if let Some(file) = &self.file {
            file.set_leadin(duration);
            self.leadin = duration;
        }
    }

    pub fn leadin(&self) -> Duration {
        self.leadin
    }

    pub fn build_effects(&mut self, chart: &Chart) {
        let offset = Duration::from_millis(chart.audio.bgm.offset.max(0) as _);
        let neg_offset = Duration::from_millis(chart.audio.bgm.offset.min(0).unsigned_abs() as _);

        let Some(sample_rate) = self.file.as_ref().map(|x| x.sample_rate) else {
            return;
        };
        let Some(channels) = self.file.as_ref().map(|x| x.channels) else {
            return;
        };

        //TODO: Clean up
        //TODO: Effect priority
        self.effects = chart
            .get_effect_tracks()
            .into_iter()
            .map(|x| vec![x])
            .coalesce(|mut a, mut b| {
                let b = b.remove(0);
                if a.iter().any(|x| x.interval.overlaps(&b.interval)) {
                    a.push(b);
                    Ok(a)
                } else {
                    Err((a, vec![b]))
                }
            })
            .map(|effect_part| {
                let start_tick = effect_part
                    .iter()
                    .map(|x| x.interval.y)
                    .min()
                    .unwrap_or_default();
                let end_tick = effect_part
                    .iter()
                    .map(|x| x.interval.y + x.interval.l)
                    .max()
                    .unwrap_or_default();

                let section_start_ms = chart.tick_to_ms(start_tick);
                let section_end_ms = chart.tick_to_ms(end_tick);
                let offset_ms = offset.as_millis() as f64 - neg_offset.as_millis() as f64;

                let start_pos = (section_start_ms + offset_ms)
                    * (sample_rate.get() as f64 / 1000.0)
                    * channels.get() as f64;
                let end_pos = (section_end_ms + offset_ms)
                    * (sample_rate.get() as f64 / 1000.0)
                    * channels.get() as f64;

                let effect_part = effect_part
                    .into_iter()
                    .map(|x| {
                        (
                            (
                                chart.tick_to_ms(x.interval.y) - section_start_ms,
                                chart.tick_to_ms(x.interval.y + x.interval.l) - section_start_ms,
                                chart.bpm_at_tick(x.interval.y),
                                matches!(x.track, Some(kson::Track::FX(_))),
                            ),
                            x.effect,
                        )
                    })
                    .collect_vec();
                (
                    (start_pos as u64, end_pos as u64),
                    Box::new(move |base| {
                        effect_part
                            .iter()
                            .fold(base, |base, ((start_ms, end_ms, bpm, is_fx), effect)| {
                                let start = Duration::from_nanos((start_ms * 1000000.0) as _);
                                let end = Duration::from_nanos((end_ms * 1000000.0) as _);
                                let duration = end - start;
                                let effected = build_effect(base, effect, start, duration, *bpm as f32, *is_fx);
                                let mix = effect.mix().map_or(1.0, |m| {
                                    m.interpolate(effect_position(*is_fx), true).clamp(0.0, 1.0)
                                });
                                Box::new(effected_part(effected, start, duration, mix))
                                    as Box<dyn Source<Item = f32> + Send>
                            }) as Box<dyn Source<Item = f32> + Send>
                    }) as Box<EffectBuilder>,
                )
            })
            .collect();
    }

    pub fn get_ms(&self) -> f64 {
        if let Some(file) = &self.file {
            file.get_ms() + (self.leadin.as_secs_f64() * 1000.0)
        } else {
            0.0
        }
    }

    pub fn get_tick(&self, chart: &Chart) -> f64 {
        if self.is_playing() {
            let ms = self.get_ms();
            let offset = chart.audio.bgm.offset;
            let ms = ms - offset as f64;
            chart.ms_to_tick(ms) as f64
        } else {
            0.0
        }
    }

    pub fn is_playing(&self) -> bool {
        match &self.file {
            Some(f) => !f.stopped.load(Ordering::SeqCst),
            None => false,
        }
    }

    pub fn get_source(&mut self) -> Option<AudioFile> {
        if let Some(file) = self.file.as_ref() {
            Some(AudioFile {
                audio: file.audio.clone(),
                audio_base: file.audio_base.clone(),
                effected: file.effected.clone(),
                effected_base: file.effected_base.clone(),
                leadin: file.leadin.clone(),
                stopped: file.stopped.clone(),
                fx_enable: file.fx_enable.clone(),
                channels: file.channels,
                sample_rate: file.sample_rate,
                pos: file.pos.clone(),
                effects: std::mem::take(&mut self.effects).into_iter().collect(),
                active_effects: vec![],
            })
        } else {
            None
        }
    }

    pub fn play(&mut self) -> bool {
        if self.is_playing() {
            true
        } else {
            if let Some(file) = &mut self.file {
                file.pos.store(0, Ordering::Relaxed);
                file.set_stopped(false);
                return true;
            }

            false
        }
    }
    pub fn open(
        &mut self,
        source: Box<dyn Source<Item = f32> + Send>,
        filename: &str,
        effected: Option<Box<dyn Source<Item = f32> + Send>>,
    ) -> Result<()> {
        let rate = source.sample_rate();
        let channels = source.channels();

        let effected: Option<SkipDuration<Buffered<Box<dyn Source<Item = f32> + Send>>>> =
            effected.map(|e| e.buffered().skip_duration(Duration::ZERO));
        let audio = source.buffered().skip_duration(Duration::ZERO);
        self.file = Some(AudioFile {
            audio: audio.clone(),
            audio_base: audio,
            effected: effected.clone(),
            effected_base: effected,
            leadin: Arc::new(AtomicUsize::new(0)),
            stopped: Arc::new(AtomicBool::new(false)),
            fx_enable: [
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(false)),
            ],
            channels,
            sample_rate: rate,
            pos: Arc::new(AtomicUsize::new(0)),
            effects: VecDeque::new(),
            active_effects: vec![],
        });
        self.last_file = filename.to_string();
        Ok(())
    }

    pub fn open_path(&mut self, path: &str) -> Result<()> {
        let new_file = String::from(path);
        if self.file.is_some() && self.last_file.eq(&new_file) {
            //don't reopen already opened file
            return Ok(());
        }

        self.close();
        let file = File::open(path)?;
        let source = rodio::Decoder::new(BufReader::new(file))?;
        self.open(
            Box::new(source),
            path,
            None as Option<Box<dyn Source + Send>>,
        )
    }

    pub fn stop(&mut self) {
        if let Some(file) = &mut self.file {
            file.set_stopped(true);
        }
    }

    //release trhe currently loaded file
    pub fn close(&mut self) {
        self.stop();
        if let Some(mut file) = self.file.take() {
            file.set_stopped(true);
        }
    }

    pub fn release(&mut self) {
        self.close();
    }
}

impl Default for AudioPlayback {
    fn default() -> Self {
        Self::new()
    }
}

/// The parameter position used for pre-built effect sections.
///
/// Long FX notes use the OnMin value (spec: "Audio effect parameter value format").
/// Laser effects would follow the laser position, which pre-built sections cannot do,
/// so they use OnMax.
fn effect_position(is_fx: bool) -> f32 {
    if is_fx {
        0.0
    } else {
        1.0
    }
}

/// Samples at the spec's reference sampling rate of 44100Hz to a duration
fn samples_to_duration(samples: i64) -> Duration {
    Duration::from_secs_f64(samples.max(0) as f64 / 44100.0)
}

/// Builds the effect source for one effect interval. Effects that are bypassed by their
/// parameters, or not supported here, pass the audio through.
fn build_effect(
    base: Box<dyn Source<Item = f32> + Send>,
    effect: &kson::effects::AudioEffect,
    start: Duration,
    duration: Duration,
    bpm: f32,
    is_fx: bool,
) -> Box<dyn MixSource<Item = f32> + Send> {
    use kson::effects::AudioEffect;
    let v = effect_position(is_fx);
    let length = |p: &kson::parameter::EffectParameter<f32>| p.to_duration(bpm, v, true);

    match effect {
        AudioEffect::Retrigger(r) => {
            let wave_length = length(&r.wave_length);
            // `wave_length` 0: the effect is bypassed
            if wave_length.is_zero() {
                return Box::new(NoMix(base));
            }
            Box::new(re_trigger(
                base,
                start,
                wave_length,
                length(&r.update_period),
                1.0,
            ))
        }
        AudioEffect::Gate(g) => {
            let wave_length = length(&g.wave_length);
            if wave_length.is_zero() {
                return Box::new(NoMix(base));
            }
            // The gate switches the volume between 100% and 0%; `rate` is the audible part
            Box::new(gate(
                base,
                start,
                wave_length,
                g.rate.interpolate(v, true) as f64,
                0.0,
            ))
        }
        AudioEffect::Flanger(f) => {
            let period = length(&f.period).as_secs_f32();
            // The flanger sweeps the delay from 0 up to its depth, so `delay` + `depth`
            // is used as the maximum delay
            let max_delay = f.delay.interpolate(v, true) + f.depth.interpolate(v, true);
            Box::new(flanger(
                base,
                samples_to_duration(max_delay),
                start,
                // `period` 0: the LFO phase stays at its current value
                if period > 0.0 { 1.0 / period } else { 0.0 },
                f.stereo_width.interpolate(v, true),
            ))
        }
        AudioEffect::PitchShift(p) => Box::new(pitch_shift(
            base,
            p.pitch.interpolate(v, true).round() as i32,
        )),
        AudioEffect::BitCrusher(b) => Box::new(bit_crusher(
            base,
            b.reduction.interpolate(v, true).max(0) as u32,
        )),
        AudioEffect::Phaser(p) => Box::new(phaser(
            base,
            PhaserParams {
                stages: p.stage.interpolate(v, true).clamp(0, 12) as usize,
                period: length(&p.period),
                freq_1: p.freq_1.interpolate(v, true),
                freq_2: p.freq_2.interpolate(v, true),
                q: p.q.interpolate(v, true),
                feedback: p.feedback.interpolate(v, true),
                stereo_width: p.stereo_width.interpolate(v, true),
                hi_cut_gain: p.hi_cut_gain.interpolate(v, true),
            },
        )),
        AudioEffect::Wobble(w) => {
            let wave_length = length(&w.wave_length);
            if wave_length.is_zero() {
                return Box::new(NoMix(base));
            }
            Box::new(wobble(
                base,
                1.0 / wave_length.as_secs_f32(),
                w.freq_1.interpolate(v, true),
                w.freq_2.interpolate(v, true),
            ))
        }
        AudioEffect::Tapestop(_) => Box::new(tape_stop(base, start, duration)),
        AudioEffect::Echo(e) => {
            let wave_length = length(&e.wave_length);
            if wave_length.is_zero() {
                return Box::new(NoMix(base));
            }
            Box::new(re_trigger(
                base,
                start,
                wave_length,
                length(&e.update_period),
                e.feedback_level.interpolate(v, true).clamp(0.0, 1.0),
            ))
        }
        AudioEffect::Sidechain(s) => Box::new(side_chain(
            base,
            start,
            length(&s.period),
            length(&s.attack_time),
            length(&s.hold_time),
            // Chart::get_effect_tracks fills in the chart-dependent default
            s.release_time_or_default(false).to_duration(bpm, v, true),
            s.ratio.interpolate(v, true),
        )),
        // Filters on laser notes are driven live by the game from the laser position
        AudioEffect::HighPassFilter(_)
        | AudioEffect::LowPassFilter(_)
        | AudioEffect::PeakingFilter(_)
            if is_fx =>
        {
            match effect.get_biquad_state(v) {
                Some(state) => Box::new(biquad(base, state, None)),
                None => Box::new(NoMix(base)),
            }
        }
        // switch_audio needs a second audio file and is not supported here
        _ => Box::new(NoMix(base)),
    }
}

pub trait GetBiQuadState {
    /// Filter state at the parameter position `v` (0.0-1.0, the laser position)
    fn get_biquad_state(&self, v: f32) -> Option<kson_rodio_sources::biquad::BiQuadState>;
}

/// Frequency for the envelope value `v`: `freq` at 0.0 and `freq_max` at 1.0, on a log scale
/// (spec: filter `v`)
fn filter_freq(freq: f32, freq_max: f32, v: f32) -> f32 {
    let (freq, freq_max) = (freq.max(1.0), freq_max.max(1.0));
    freq * (freq_max / freq).powf(v.clamp(0.0, 1.0))
}

/// Q for a bandwidth in octaves (Audio EQ Cookbook)
fn q_from_bandwidth(bandwidth: f32) -> f32 {
    let bandwidth = bandwidth.max(0.01);
    1.0 / (2.0 * (std::f32::consts::LN_2 / 2.0 * bandwidth).sinh())
}

impl GetBiQuadState for kson::effects::AudioEffect {
    fn get_biquad_state(&self, p: f32) -> Option<kson_rodio_sources::biquad::BiQuadState> {
        use kson_rodio_sources::biquad::BiQuadState;
        use kson_rodio_sources::biquad::BiQuadType;
        match self {
            kson::effects::AudioEffect::HighPassFilter(f) => Some(BiQuadState::new(
                BiQuadType::HighPass,
                f.q.interpolate(p, true),
                filter_freq(
                    f.freq.interpolate(p, true),
                    f.freq_max.interpolate(p, true),
                    f.v.interpolate(p, true),
                ),
            )),
            kson::effects::AudioEffect::LowPassFilter(f) => Some(BiQuadState::new(
                BiQuadType::LowPass,
                f.q.interpolate(p, true),
                filter_freq(
                    f.freq.interpolate(p, true),
                    f.freq_max.interpolate(p, true),
                    f.v.interpolate(p, true),
                ),
            )),
            kson::effects::AudioEffect::PeakingFilter(f) => Some(BiQuadState::new(
                BiQuadType::Peaking(f.gain.interpolate(p, true) * 20.0),
                q_from_bandwidth(f.bandwidth.interpolate(p, true)),
                filter_freq(
                    f.freq.interpolate(p, true),
                    f.freq_max.interpolate(p, true),
                    f.v.interpolate(p, true),
                ),
            )),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::source::SineWave;

    const EFFECTS: [&str; 13] = [
        "retrigger",
        "gate",
        "flanger",
        "pitch_shift",
        "bitcrusher",
        "phaser",
        "wobble",
        "tapestop",
        "echo",
        "sidechain",
        "peaking_filter",
        "high_pass_filter",
        "low_pass_filter",
    ];

    fn run(effect: &kson::effects::AudioEffect, is_fx: bool) -> Vec<f32> {
        let base: Box<dyn Source<Item = f32> + Send> =
            Box::new(SineWave::new(440.0).take_duration(Duration::from_millis(500)));
        let mut effected = build_effect(
            base,
            effect,
            Duration::from_millis(50),
            Duration::from_millis(400),
            120.0,
            is_fx,
        );
        effected.set_mix(1.0);
        effected.collect()
    }

    #[test]
    fn builtin_effects_do_not_panic_or_produce_nan() {
        for name in EFFECTS {
            let mut effect = kson::effects::AudioEffect::builtin(name).unwrap();
            if let kson::effects::AudioEffect::Sidechain(s) = &mut effect {
                s.release_time = Some(s.release_time_or_default(false));
            }
            for is_fx in [true, false] {
                let out = run(&effect, is_fx);
                assert!(
                    out.iter().all(|s| s.is_finite()),
                    "{name} (fx: {is_fx}) produced a non-finite sample"
                );
            }
        }
    }

    #[test]
    fn effects_with_wave_length_are_active() {
        for name in ["retrigger", "gate", "wobble", "echo"] {
            let json = format!(r#"{{"type":"{name}","v":{{"wave_length":"1/8"}}}}"#);
            let effect: kson::effects::AudioEffect = serde_json::from_str(&json).unwrap();
            let out = run(&effect, true);
            assert!(out.iter().all(|s| s.is_finite()), "{name}");
        }
    }

    #[test]
    fn filter_frequency_follows_v() {
        assert!((filter_freq(100.0, 2200.0, 0.0) - 100.0).abs() < 0.01);
        assert!((filter_freq(100.0, 2200.0, 1.0) - 2200.0).abs() < 0.1);
        // Log scale: the middle is the geometric mean
        assert!((filter_freq(100.0, 10000.0, 0.5) - 1000.0).abs() < 0.1);
        // One octave of bandwidth is a Q of about 1.414
        assert!((q_from_bandwidth(1.0) - 1.414).abs() < 0.01);
    }
}

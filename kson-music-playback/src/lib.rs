use anyhow::Result;
use itertools::Itertools;
use kson::overlaps::Overlaps;
use kson::Chart;

use rodio::source::{Buffered, SkipDuration};
pub use rodio::Source;

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
    mix_source::{MixSource, NoMix},
    pitch_shift::pitch_shift,
    re_trigger::re_trigger,
    side_chain::side_chain,
    tape_stop::tape_stop,
    wobble::wobble,
};

type ActiveEffect = ((u64, u64), Box<dyn Source<Item = f32> + Send>);

pub struct AudioFile {
    audio: SkipDuration<Buffered<Box<dyn Source<Item = f32> + Send>>>,
    audio_base: SkipDuration<Buffered<Box<dyn Source<Item = f32> + Send>>>,
    effected: Option<SkipDuration<Buffered<Box<dyn Source<Item = f32> + Send>>>>,
    effected_base: Option<SkipDuration<Buffered<Box<dyn Source<Item = f32> + Send>>>>,
    leadin: Arc<AtomicUsize>,
    stopped: Arc<AtomicBool>,
    fx_enable: [Arc<AtomicBool>; 2],
    channels: u16,
    sample_rate: u32,
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
    fn current_frame_len(&self) -> Option<usize> {
        self.audio.current_frame_len()
    }

    #[inline]
    fn channels(&self) -> u16 {
        self.channels
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
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
            -((leadin / self.channels as usize) as f64 / (self.sample_rate as f64 / 1000.0))
        } else {
            (self.pos.load(Ordering::SeqCst) / self.channels as usize) as f64
                / (self.sample_rate as f64 / 1000.0)
        }
    }

    fn set_stopped(&mut self, val: bool) {
        self.stopped.store(val, Ordering::SeqCst);
    }

    fn set_leadin(&self, duration: Duration) {
        self.leadin.store(
            ((duration.as_millis() * self.sample_rate as u128) / 1000) as usize
                * self.channels as usize,
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
}

impl AudioPlayback {
    pub fn new() -> Self {
        AudioPlayback {
            file: None,
            last_file: String::new(),
            effects: vec![],
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
            file.set_leadin(duration)
        }
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
                    * (sample_rate as f64 / 1000.0)
                    * channels as f64;
                let end_pos =
                    (section_end_ms + offset_ms) * (sample_rate as f64 / 1000.0) * channels as f64;

                let effect_part = effect_part
                    .into_iter()
                    .map(|x| {
                        (
                            (
                                chart.tick_to_ms(x.interval.y) - section_start_ms,
                                chart.tick_to_ms(x.interval.y + x.interval.l) - section_start_ms,
                                chart.bpm_at_tick(x.interval.y),
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
                            .fold(base, |base, ((start_ms, end_ms, bpm), effect)| {
                                let start = Duration::from_nanos((start_ms * 1000000.0) as _);
                                let end = Duration::from_nanos((end_ms * 1000000.0) as _);
                                let duration = end - start;
                                let bpm = *bpm;
                                let effected: Box<dyn MixSource<Item = f32> + Send> = match effect {
                                    kson::effects::AudioEffect::ReTrigger(r) => {
                                        let duration = Duration::from_secs_f64(
                                            (240.0 * r.wave_length.interpolate(1.0, true) as f64)
                                                / bpm,
                                        );

                                        let update_duration = Duration::from_secs_f64(
                                            (240.0 * r.update_period.interpolate(1.0, true) as f64)
                                                / bpm,
                                        );
                                        Box::new(re_trigger(
                                            base,
                                            start,
                                            duration,
                                            update_duration,
                                            1.0,
                                        ))
                                    }
                                    kson::effects::AudioEffect::Gate(g) => {
                                        let period = Duration::from_secs_f64(
                                            (240.0 * g.wave_length.interpolate(1.0, true) as f64)
                                                / bpm,
                                        );
                                        Box::new(gate(base, start, period, 0.6, 0.4))
                                    }
                                    kson::effects::AudioEffect::Flanger(_f) => Box::new(flanger(
                                        base,
                                        Duration::from_millis(4),
                                        Duration::from_millis(1),
                                        0.5,
                                        0.05,
                                    )),
                                    kson::effects::AudioEffect::PitchShift(p) => Box::new(
                                        pitch_shift(base, p.pitch.interpolate(1.0, true) as _),
                                    ),
                                    kson::effects::AudioEffect::BitCrusher(b) => Box::new(
                                        bit_crusher(base, b.reduction.interpolate(1.0, true) as _),
                                    ),
                                    kson::effects::AudioEffect::Phaser(_p) => Box::new(
                                        //TODO
                                        flanger(
                                            base,
                                            Duration::from_millis(4),
                                            Duration::from_millis(1),
                                            0.5,
                                            0.05,
                                        ),
                                    ),
                                    kson::effects::AudioEffect::Wobble(w) => Box::new(wobble(
                                        base,
                                        1.0 / ((240.0 * w.wave_length.interpolate(1.0, true))
                                            / bpm as f32),
                                        w.lo_freq.interpolate(1.0, true) as _,
                                        w.hi_freq.interpolate(1.0, true) as _,
                                    )),
                                    kson::effects::AudioEffect::TapeStop(_t) => {
                                        Box::new(tape_stop(base, start, duration))
                                    }
                                    kson::effects::AudioEffect::Echo(r) => {
                                        let duration = Duration::from_secs_f64(
                                            (240.0 * r.wave_length.interpolate(1.0, true) as f64)
                                                / bpm,
                                        );
                                        let feedback =
                                            r.feedback_level.interpolate(1.0, true).clamp(0.0, 1.0);

                                        Box::new(re_trigger(
                                            base,
                                            start,
                                            duration,
                                            Duration::ZERO,
                                            feedback,
                                        ))
                                    }
                                    kson::effects::AudioEffect::SideChain(s) => {
                                        let bpm = bpm as f32;

                                        Box::new(side_chain(
                                            base,
                                            start,
                                            s.period.to_duration(bpm, 1.0, true),
                                            s.attack_time.to_duration(bpm, 1.0, true),
                                            s.hold_time.to_duration(bpm, 1.0, true),
                                            s.release_time.to_duration(bpm, 1.0, true),
                                            s.ratio.interpolate(1.0, true),
                                        ))
                                    }
                                    _ => Box::new(NoMix(base)),
                                };
                                Box::new(effected_part(effected, start, duration, 1.0))
                                    as Box<dyn Source<Item = f32> + Send>
                            }) as Box<dyn Source<Item = f32> + Send>
                    }) as Box<EffectBuilder>,
                )
            })
            .collect();
    }

    pub fn get_ms(&self) -> f64 {
        if let Some(file) = &self.file {
            file.get_ms()
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
            Box::new(source.convert_samples()),
            path,
            None as Option<Box<dyn Source<Item = f32> + Send>>,
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

pub trait GetBiQuadState {
    fn get_biquad_state(&self, v: f32) -> Option<kson_rodio_sources::biquad::BiQuadState>;
}

impl GetBiQuadState for kson::effects::AudioEffect {
    fn get_biquad_state(&self, p: f32) -> Option<kson_rodio_sources::biquad::BiQuadState> {
        use kson_rodio_sources::biquad::BiQuadState;
        use kson_rodio_sources::biquad::BiQuadType;
        match self {
            kson::effects::AudioEffect::HighPassFilter(v) => Some(BiQuadState::new(
                BiQuadType::HighPass,
                v.q.interpolate(p, true),
                v.freq.interpolate(p, true),
            )),
            kson::effects::AudioEffect::LowPassFilter(v) => Some(BiQuadState::new(
                BiQuadType::LowPass,
                v.q.interpolate(p, true),
                v.freq.interpolate(p, true),
            )),
            kson::effects::AudioEffect::PeakingFilter(v) => Some(BiQuadState::new(
                BiQuadType::Peaking(v.gain.interpolate(p, true) * 20.0),
                v.q.interpolate(p, true),
                v.freq.interpolate(p, true),
            )),
            _ => None,
        }
    }
}

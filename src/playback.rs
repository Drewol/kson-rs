use crate::chart::{Chart, GraphSectionPoint};
use crate::dsp;
use ggez::GameResult;
use rodio::*;
use std::fs::File;
use std::io::BufReader;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use time_calc::tick_in_ms;

#[derive(Clone)]
pub struct AudioFile {
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
    size: usize,
    pos: Arc<Mutex<usize>>,
    stopped: Arc<AtomicBool>,
    laser_dsp: Arc<Mutex<dyn dsp::LaserEffect>>,
}

impl Iterator for AudioFile {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        {
            if self.stopped.load(Ordering::SeqCst) {
                return None;
            }
        }
        {
            let mut pos = self.pos.lock().unwrap();
            let samples = self.samples.lock().unwrap();

            if *pos >= self.size {
                None
            } else {
                let mut v = *(*samples).get(*pos).unwrap();
                v = v * 0.6;
                *pos = *pos + 1;

                //apply DSPs

                //apply Laser DSP
                {
                    let mut laser = self.laser_dsp.lock().unwrap();
                    (*laser).process(&mut v, *pos % self.channels as usize);
                }

                Some(v)
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.size - 1))
    }
}

impl source::Source for AudioFile {
    fn current_frame_len(&self) -> Option<usize> {
        let pos = self.pos.lock().unwrap();
        if *pos == self.size {
            Some(0)
        } else {
            Some(32.min(self.size - *pos))
        }
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
        Some(std::time::Duration::from_secs_f64(
            (self.size / self.channels as usize) as f64 / self.sample_rate as f64,
        ))
    }
}

impl AudioFile {
    fn get_ms(&self) -> f64 {
        let pos = self.pos.lock().unwrap();
        (*pos / self.channels as usize) as f64 / (self.sample_rate as f64 / 1000.0)
    }

    fn set_ms(&mut self, ms: f64) {
        let mut pos = self.pos.lock().unwrap();
        *pos = ((ms / 1000.0) * (self.sample_rate * self.channels as u32) as f64) as usize;
        *pos = *pos - (*pos % self.channels as usize);
    }

    fn set_stopped(&mut self, val: bool) {
        self.stopped.store(val, Ordering::SeqCst);
    }
}

pub struct AudioPlayback {
    sink: Sink,
    file: Option<AudioFile>,
    last_file: String,
    laser_funcs: [Vec<(u32, u32, Box<dyn Fn(f32) -> f32>)>; 2],
    laser_values: (Option<f32>, Option<f32>),
}

impl AudioPlayback {
    pub fn new(ctx: &ggez::Context) -> Self {
        AudioPlayback {
            sink: Sink::new(ctx.audio_context.device()),
            file: None,
            last_file: String::new(),
            laser_funcs: [Vec::new(), Vec::new()],
            laser_values: (None, None),
        }
    }

    fn make_laser_fn(
        base_y: u32,
        p1: &GraphSectionPoint,
        p2: &GraphSectionPoint,
    ) -> Box<dyn Fn(f32) -> f32> {
        let start_tick = (base_y + p1.ry) as f32;
        let end_tick = (base_y + p2.ry) as f32;
        let start_value = match p1.vf {
            Some(v) => v,
            None => p1.v,
        } as f32;
        let end_value = p2.v as f32;
        let value_delta = end_value - start_value;
        let length = end_tick - start_tick;

        if start_value == end_value {
            Box::new(move |_: f32| start_value)
        } else {
            Box::new(move |y: f32| start_value + value_delta * ((y - start_tick) / length))
        }
    }

    pub fn build_effects(&mut self, chart: &Chart) {
        for i in 0..2 {
            self.laser_funcs[i].clear();
            for section in &chart.note.laser[i] {
                for se in section.v.windows(2) {
                    let s = section.y + se[0].ry;
                    let e = section.y + se[1].ry;
                    self.laser_funcs[i].push((
                        s,
                        e,
                        AudioPlayback::make_laser_fn(section.y, &se[0], &se[1]),
                    ));
                }
            }
        }
    }

    pub fn get_ms(&self) -> f64 {
        if let Some(file) = &self.file {
            file.get_ms()
        } else {
            0.0
        }
    }

    pub fn get_tick(&self, chart: &Chart) -> u32 {
        if self.is_playing() {
            let ms = self.get_ms();
            let offset = match &chart.audio.bgm {
                Some(bgm) => bgm.offset,
                None => 0,
            };
            let ms = ms - offset as f64;
            chart.ms_to_tick(ms)
        } else {
            0
        }
    }

    pub fn is_playing(&self) -> bool {
        !self.sink.empty()
    }

    fn get_laser_value_at(&self, side: usize, tick: f32) -> Option<f32> {
        let utick = tick as u32;
        for (s, e, f) in &self.laser_funcs[side] {
            if utick < *s {
                return None;
            }
            if utick > *e {
                continue;
            }
            if utick <= *e && utick >= *s {
                let v = f(tick);
                if side == 1 {
                    return Some(1.0 - v);
                } else {
                    return Some(v);
                }
            }
        }

        None
    }

    pub fn update(&mut self, chart: &Chart, tick: u32) {
        if !self.is_playing() {
            return;
        }

        let offset = match &chart.audio.bgm {
            Some(bgm) => bgm.offset,
            None => 0,
        };
        let bpm = chart.bpm_at_tick(tick);
        let tick_length = tick_in_ms(bpm, chart.beat.resolution);
        let ms = self.get_ms() - offset as f64;
        let ms = ms - chart.tick_to_ms(tick);
        let tick = tick as f32 + (ms / tick_length) as f32;

        self.laser_values = (
            self.get_laser_value_at(0, tick),
            self.get_laser_value_at(1, tick),
        );

        let dsp_value = match self.laser_values {
            (Some(v1), Some(v2)) => Some(v1.max(v2)),
            (Some(v), None) => Some(v),
            (None, Some(v)) => Some(v),
            (None, None) => None,
        };

        if self.is_playing() {
            if let Some(file) = &mut self.file {
                if let Some(dsp_value) = dsp_value {
                    let mut laser = file.laser_dsp.lock().unwrap();
                    laser.set_mix(1.0);
                    laser.set_laser_value(dsp_value);
                } else {
                    let mut laser = file.laser_dsp.lock().unwrap();
                    laser.set_mix(0.0);
                }
            }
        }
    }

    pub fn get_laser_values(&self) -> (Option<f32>, Option<f32>) {
        self.laser_values
    }

    pub fn play(&mut self) -> bool {
        if self.is_playing() {
            true
        } else {
            if let Some(file) = &mut self.file {
                file.set_stopped(false);
                self.sink.append(file.clone());
                return true;
            }

            false
        }
    }

    pub fn open(&mut self, path: &str) -> GameResult {
        let new_file = String::from(path);
        if let Some(_) = &self.file {
            if self.last_file.eq(&new_file) {
                //don't reopen already opened file
                return Ok(());
            }
        }

        self.close();
        let file = File::open(path)?;
        let source = match rodio::Decoder::new(BufReader::new(file)) {
            Ok(s) => s,
            Err(_) => {
                return Err(ggez::GameError::AudioError(
                    "Failed to create decoder.".to_owned(),
                ))
            }
        };
        let rate = source.sample_rate();
        let channels = source.channels();
        let dataref: Arc<Mutex<Vec<f32>>> =
            Arc::new(Mutex::new(source.convert_samples().collect()));
        let data = dataref.lock().unwrap();

        self.file = Some(AudioFile {
            size: (*data).len(),
            samples: dataref.clone(),
            sample_rate: rate,
            channels: channels,
            pos: Arc::new(Mutex::new(0)),
            stopped: Arc::new(AtomicBool::new(false)),
            laser_dsp: Arc::new(Mutex::new(dsp::BiQuad::new(
                dsp::BiQuadType::Peaking(10.0),
                rate,
                200.0,
                16000.0,
                1.0,
                channels as usize,
            ))),
        });
        self.last_file = new_file;
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(file) = &mut self.file {
            file.set_stopped(true);
        }
    }

    //release trhe currently loaded file
    pub fn close(&mut self) {
        self.stop();
        if let Some(_) = &self.file {
            self.file = None;
        }
    }

    pub fn release(&mut self) {
        self.close();
    }

    pub fn set_poistion(&mut self, ms: f64) {
        if let Some(file) = &mut self.file {
            file.set_ms(ms);
        }
    }
}

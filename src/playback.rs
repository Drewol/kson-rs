use crate::dsp;
use anyhow::Result;
use kson::{Chart, GraphSectionPoint};
use rodio::*;
use std::fs::File;
use std::io::BufReader;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AudioFile {
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
    size: usize,
    pos: Arc<AtomicUsize>,
    stopped: Arc<AtomicBool>,
    laser_dsp: Arc<Mutex<dyn dsp::Dsp>>,
    fx_dsp: [Option<Arc<Mutex<dyn dsp::Dsp>>>; 2],
    fx_enable: [Arc<AtomicBool>; 2],
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
        {
            if self.stopped.load(Ordering::SeqCst) {
                return None;
            }
        }
        {
            let mut pos = self.pos.load(Ordering::SeqCst);
            let samples = self.samples.lock().unwrap();

            if pos >= self.size {
                None
            } else {
                let mut v = *(*samples).get(pos).unwrap();
                v *= 0.6;
                pos += 1;

                //apply DSPs
                for i in 0..2 {
                    let d = &self.fx_dsp[i];
                    let en = &self.fx_enable[i];
                    if en.load(Ordering::SeqCst) {
                        if let Some(d) = d {
                            let mut d = d.lock().unwrap();
                            d.process(&mut v, pos % self.channels as usize);
                        }
                    }
                }

                //apply Laser DSP
                {
                    let mut laser = self.laser_dsp.lock().unwrap();
                    (*laser).process(&mut v, pos % self.channels as usize);
                }
                self.pos.store(pos, Ordering::SeqCst);
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
        let pos = self.pos.load(Ordering::SeqCst);
        if pos == self.size {
            Some(0)
        } else {
            Some(32.min(self.size - pos))
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
        (self.pos.load(Ordering::SeqCst) / self.channels as usize) as f64
            / (self.sample_rate as f64 / 1000.0)
    }

    fn set_ms(&mut self, ms: f64) {
        let mut pos = ((ms / 1000.0) * (self.sample_rate * self.channels as u32) as f64) as usize;
        pos -= pos % self.channels as usize;
        self.pos.store(pos, Ordering::SeqCst);
    }

    fn set_stopped(&mut self, val: bool) {
        self.stopped.store(val, Ordering::SeqCst);
    }
}
type LaserFn = Box<dyn Fn(f32) -> f32>;

pub struct AudioPlayback {
    sink: Sink,
    stream: OutputStream,
    file: Option<AudioFile>,
    last_file: String,
    laser_funcs: [Vec<(u32, u32, LaserFn)>; 2],
    laser_values: (Option<f32>, Option<f32>),
}

impl AudioPlayback {
    pub fn try_new() -> Result<Self> {
        let (stream, stream_handle) = OutputStream::try_default()?;
        let sink = Sink::try_new(&stream_handle)?;
        Ok(AudioPlayback {
            sink,
            stream,
            file: None,
            last_file: String::new(),
            laser_funcs: [Vec::new(), Vec::new()],
            laser_values: (None, None),
        })
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
        let a = p1.a.unwrap_or(0.5);
        let b = p1.b.unwrap_or(0.5);
        if (start_value - end_value).abs() < f32::EPSILON {
            Box::new(move |_: f32| start_value)
        } else if (a - b).abs() > f64::EPSILON {
            Box::new(move |y: f32| {
                let x = ((y - start_tick) / length).min(1.0).max(0.0) as f64;
                start_value + value_delta * kson::do_curve(x, a, b) as f32
            })
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

    pub fn get_tick(&self, chart: &Chart) -> f64 {
        if self.is_playing() {
            let ms = self.get_ms();
            let offset = match &chart.audio.bgm {
                Some(bgm) => bgm.offset,
                None => 0,
            };
            let ms = ms - offset as f64;
            chart.ms_to_tick(ms) as f64
        } else {
            0.0
        }
    }

    pub fn is_playing(&self) -> bool {
        !self.sink.empty()
    }

    fn get_laser_value_at(&self, side: usize, tick: f64) -> Option<f32> {
        let utick = tick as u32;
        for (s, e, f) in &self.laser_funcs[side] {
            if utick < *s {
                return None;
            }
            if utick > *e {
                continue;
            }
            if utick <= *e && utick >= *s {
                let v = f(tick as f32);
                if side == 1 {
                    return Some(1.0 - v);
                } else {
                    return Some(v);
                }
            }
        }

        None
    }

    pub fn update(&mut self, tick: f64) {
        if !self.is_playing() {
            return;
        }

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
                    laser.set_param_transition(dsp_value);
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
                self.sink.play();
                return true;
            }

            false
        }
    }

    pub fn open(&mut self, path: &str) -> Result<()> {
        let new_file = String::from(path);
        if self.file.is_some() && self.last_file.eq(&new_file) {
            //don't reopen already opened file
            return Ok(());
        }

        self.close();
        let file = File::open(path)?;
        let source = rodio::Decoder::new(BufReader::new(file))?;
        let rate = source.sample_rate();
        let channels = source.channels();
        let dataref: Arc<Mutex<Vec<f32>>> =
            Arc::new(Mutex::new(source.convert_samples().collect()));
        let data = dataref.lock().unwrap();

        self.file = Some(AudioFile {
            size: (*data).len(),
            samples: dataref.clone(),
            sample_rate: rate,
            channels,
            pos: Arc::new(AtomicUsize::new(0)),
            stopped: Arc::new(AtomicBool::new(false)),
            fx_enable: [
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(false)),
            ],
            fx_dsp: [None, None],
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
        if self.file.is_some() {
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

extern crate rfmod;

use chart::{Chart, GraphSectionPoint};
use time_calc::tick_in_ms;

pub struct AudioPlayback {
    channel: Option<rfmod::Channel>,
    sound: Option<rfmod::Sound>,
    sys: rfmod::Sys,
    last_file: String,
    laser_funcs: [Vec<(u32, u32, Box<dyn Fn(f32) -> f32>)>; 2],
    laser_values: (f32, f32),
}

impl AudioPlayback {
    pub fn new() -> Self {
        let fmod_sys = rfmod::Sys::new().unwrap();
        match fmod_sys.init() {
            rfmod::Status::Ok => {}
            e => {
                panic!("FmodSys.init failed : {:?}", e);
            }
        };

        AudioPlayback {
            sys: fmod_sys,
            sound: None,
            channel: None,
            last_file: String::new(),
            laser_funcs: [Vec::new(), Vec::new()],
            laser_values: (0.0, 0.0),
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
        if let Some(channel) = &self.channel {
            let ms = channel.get_position(rfmod::TIMEUNIT_PCM).unwrap() as f64;
            (ms / channel.get_frequency().unwrap() as f64) * 1000.0
        } else {
            0.0
        }
    }

    pub fn get_tick(&self, chart: &Chart) -> u32 {
        if let Some(channel) = &self.channel {
            let ms = channel.get_position(rfmod::TIMEUNIT_PCM).unwrap() as f64;
            let ms = (ms / channel.get_frequency().unwrap() as f64) * 1000.0;
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
        if let Some(channel) = &self.channel {
            channel.is_playing().unwrap()
        } else {
            false
        }
    }

    fn get_laser_value_at(&self, side: usize, tick: f32) -> f32 {
        let utick = tick as u32;
        for (s, e, f) in &self.laser_funcs[side] {
            if utick < *s {
                return side as f32;
            }
            if utick > *e {
                continue;
            }
            if utick <= *e && utick >= *s {
                return f(tick);
            }
        }

        side as f32
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
            1.0 - self.get_laser_value_at(1, tick),
        );
    }

    pub fn get_laser_values(&self) -> (f32, f32) {
        self.laser_values
    }

    pub fn play(&mut self) -> bool {
        if self.is_playing() {
            true
        } else {
            if let Some(sound) = &self.sound {
                self.channel = match sound.play() {
                    Ok(c) => Some(c),
                    Err(e) => {
                        println!("Failed to play sound: {:?}", e);
                        None
                    }
                };
            }

            self.channel.is_some()
        }
    }

    pub fn open(&mut self, path: &str) -> bool {
        let new_file = String::from(path);
        if let Some(_) = &self.sound {
            if self.last_file.eq(&new_file) {
                //don't reopen already opened file
                return true;
            }
        }

        self.close();
        self.sound = match self.sys.create_sound(path, None, None) {
            Ok(s) => Some(s),
            Err(e) => {
                println!("Failed to open sound file: {:?}", e);
                None
            }
        };
        self.sound.is_some()
    }

    pub fn stop(&mut self) {
        if let Some(channel) = &mut self.channel {
            channel.stop();
            channel.release();
            self.channel = None;
        }
    }

    pub fn close(&mut self) {
        self.stop();
        if let Some(sound) = &mut self.sound {
            sound.release();
            self.sound = None;
        }
    }

    pub fn release(&mut self) {
        self.close();
        self.sys.release();
    }

    pub fn set_poistion(&mut self, ms: usize) {
        if let Some(channel) = &self.channel {
            channel.set_position(ms, rfmod::TIMEUNIT_MS);
        }
    }
}

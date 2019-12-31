extern crate rfmod;

use chart::Chart;
use MainState;

pub struct AudioPlayback {
    channel: Option<rfmod::Channel>,
    sound: Option<rfmod::Sound>,
    sys: rfmod::Sys,
    last_file: String,
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
        }
    }

    pub fn get_tick(&self, state: &MainState) -> u32 {
        if let Some(channel) = &self.channel {
            let ms = channel.get_position(rfmod::TIMEUNIT_PCM).unwrap() as f64;
            let ms = (ms / channel.get_frequency().unwrap() as f64) * 1000.0;
            let offset = match &state.chart.audio.bgm {
                Some(bgm) => bgm.offset,
                None => 0,
            };
            let ms = ms - offset as f64;
            state.ms_to_tick(ms)
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

    pub fn update(&mut self, chart: &Chart, tick: u32) {
        if !self.is_playing() {
            return;
        }

        //check chart for DSP's and stuff
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

use std::sync::{Arc, Mutex, RwLock};

use crate::button_codes::{LaserState, UscButton, UscInputEvent};

pub struct InputState {
    laser_state: RwLock<LaserState>,
    gilrs: Arc<Mutex<gilrs::Gilrs>>,
}

impl InputState {
    pub fn new(gilrs: Arc<Mutex<gilrs::Gilrs>>) -> Self {
        Self {
            laser_state: RwLock::new(LaserState::default()),
            gilrs,
        }
    }

    pub fn update(&mut self, e: &UscInputEvent) {
        if let Ok(mut laser_state) = self.laser_state.write() {
            match e {
                UscInputEvent::Laser(s) => *laser_state = *s,
                UscInputEvent::Button(_, _) => {}
            }
        }
    }

    pub fn is_button_held(&self, button: UscButton) -> bool {
        if let Ok(gilrs) = self.gilrs.lock() {
            gilrs.gamepads().any(|x| match x.1.mapping_source() {
                gilrs::MappingSource::SdlMappings => x.1.is_pressed(button.into()),
                _ => x.1.state().buttons().any(|(code, data)| {
                    button.to_gilrs_code_u32() == code.into_u32() && data.is_pressed()
                }),
            })
        } else {
            false
        }
    }
}

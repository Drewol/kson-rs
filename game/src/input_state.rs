use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
    time::SystemTime,
};

use game_loop::winit::event::ElementState;
use kson::Side;

use crate::button_codes::{LaserAxis, LaserState, UscButton, UscInputEvent};

#[derive(Debug, Clone)]
pub struct InputState {
    laser_state: Arc<RwLock<LaserState>>,
    _gilrs: Arc<Mutex<gilrs::Gilrs>>,
    buttons_held: Arc<RwLock<HashMap<UscButton, SystemTime>>>,
}

impl InputState {
    pub fn new(_gilrs: Arc<Mutex<gilrs::Gilrs>>) -> Self {
        Self {
            laser_state: Arc::new(RwLock::new(LaserState::default())),
            _gilrs,
            buttons_held: Arc::new(RwLock::new(HashMap::default())),
        }
    }

    pub fn update(&mut self, e: &UscInputEvent) {
        if let Ok(mut laser_state) = self.laser_state.write() {
            match e {
                UscInputEvent::Laser(s, _) => *laser_state = *s,
                UscInputEvent::Button(_, _, _) => {}
            }
        }

        if let Ok(mut buttons_held) = self.buttons_held.write() {
            match e {
                UscInputEvent::Button(b, ElementState::Pressed, _) => {
                    //TODO: Take time as an argument for better accuracy
                    buttons_held.insert(*b, std::time::SystemTime::now());
                }
                UscInputEvent::Button(b, ElementState::Released, _) => {
                    buttons_held.remove(b);
                }
                UscInputEvent::Laser(_, _) => {}
            }
        }
    }

    /// Returns time when button was pressed if held, None if button is not held
    pub fn is_button_held(&self, button: UscButton) -> Option<SystemTime> {
        self.buttons_held
            .read()
            .ok()
            .and_then(|l| l.get(&button).copied())
    }

    pub fn get_axis(&self, side: Side) -> LaserAxis {
        self.laser_state.read().unwrap().get_axis(side)
    }
}

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
    time::SystemTime,
};


use game_loop::winit::event::ElementState;


use crate::button_codes::{LaserState, UscButton, UscInputEvent};

#[derive(Debug, Clone)]
pub struct InputState {
    laser_state: Arc<RwLock<LaserState>>,
    gilrs: Arc<Mutex<gilrs::Gilrs>>,
    buttons_held: Arc<RwLock<HashMap<UscButton, SystemTime>>>,
}

impl InputState {
    pub fn new(gilrs: Arc<Mutex<gilrs::Gilrs>>) -> Self {
        Self {
            laser_state: Arc::new(RwLock::new(LaserState::default())),
            gilrs,
            buttons_held: Arc::new(RwLock::new(HashMap::default())),
        }
    }

    pub fn update(&mut self, e: &UscInputEvent) {
        if let Ok(mut laser_state) = self.laser_state.write() {
            match e {
                UscInputEvent::Laser(s) => *laser_state = *s,
                UscInputEvent::Button(_, _) => {}
            }
        }

        if let Ok(mut buttons_held) = self.buttons_held.write() {
            match e {
                UscInputEvent::Button(b, ElementState::Pressed) => {
                    //TODO: Take time as an argument for better accuracy
                    buttons_held.insert(*b, std::time::SystemTime::now());
                }
                UscInputEvent::Button(b, ElementState::Released) => {
                    buttons_held.remove(b);
                }
                UscInputEvent::Laser(_) => {}
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
}

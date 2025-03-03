use std::{
    collections::HashMap,
    sync::{atomic::AtomicBool, Arc, Mutex, RwLock},
    time::SystemTime,
};

use winit::event::ElementState;
use kson::Side;

use crate::button_codes::{LaserAxis, LaserState, UscButton, UscInputEvent};

#[derive(Debug, Clone)]
pub struct InputState {
    text_input_active: Arc<AtomicBool>,
    laser_state: Arc<RwLock<LaserState>>,
    gilrs: Arc<Mutex<gilrs::Gilrs>>,
    buttons_held: Arc<RwLock<HashMap<UscButton, SystemTime>>>,
}

impl InputState {
    pub fn new(gilrs: Arc<Mutex<gilrs::Gilrs>>) -> Self {
        Self {
            text_input_active: Arc::new(AtomicBool::new(false)),
            laser_state: Arc::new(RwLock::new(LaserState::default())),
            gilrs,
            buttons_held: Arc::new(RwLock::new(HashMap::default())),
        }
    }

    pub fn update(&mut self, e: &UscInputEvent) {
        if let Ok(mut laser_state) = self.laser_state.write() {
            match e {
                UscInputEvent::Laser(s, _) => *laser_state = *s,
                UscInputEvent::Button(_, _, _) => {}
                UscInputEvent::ClientEvent(_) => {}
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
                UscInputEvent::ClientEvent(_) => {}
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
        self.laser_state.read().expect("Lock error").get_axis(side)
    }

    pub fn lock_gilrs(&self) -> std::sync::MutexGuard<'_, gilrs::Gilrs> {
        self.gilrs.lock().expect("Lock error")
    }

    pub fn text_input_active(&self) -> bool {
        self.text_input_active
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_text_input_active(&mut self, text_input_active: bool) {
        self.text_input_active
            .store(text_input_active, std::sync::atomic::Ordering::Relaxed);
    }
}

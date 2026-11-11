use crate::button_codes::LaserState;
use crate::songselect::KNOB_NAV_THRESHOLD;

#[derive(Debug, Clone, Copy)]
pub struct LaserNavigation {
    x_acc: f32,
    y_acc: f32,
}

impl LaserNavigation {
    pub fn new() -> Self {
        Self {
            x_acc: 0.0,
            y_acc: 0.0,
        }
    }

    pub fn poll_y(&mut self) -> i8 {
        let settings_steps = (self.y_acc / KNOB_NAV_THRESHOLD).trunc() as i8;
        self.y_acc -= settings_steps as f32 * KNOB_NAV_THRESHOLD;
        settings_steps
    }
    pub fn poll_x(&mut self) -> i8 {
        let settings_steps = (self.x_acc / KNOB_NAV_THRESHOLD).trunc() as i8;
        self.x_acc -= settings_steps as f32 * KNOB_NAV_THRESHOLD;
        settings_steps
    }

    pub fn update(&mut self, ls: LaserState) {
        self.x_acc += ls.get_axis(kson::Side::Left).delta;
        self.y_acc += ls.get_axis(kson::Side::Right).delta;
    }
}

impl Default for LaserNavigation {
    fn default() -> Self {
        Self::new()
    }
}

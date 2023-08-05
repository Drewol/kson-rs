use crate::{
    button_codes::{UscButton, UscInputEvent},
    input_state::InputState,
};

type Setter<T> = Box<dyn Fn(T)>;
type Getter<T> = Box<dyn Fn() -> T>;

pub enum SettingsDialogSetting {
    Float {
        min: f32,
        max: f32,
        mult: f32,
        set: Setter<f32>,
        get: Getter<f32>,
    },
    Int {
        min: i32,
        max: i32,
        step: i32,
        div: i32,
        set: Setter<i32>,
        get: Getter<i32>,
    },

    Enum {
        options: Vec<String>,
        set: Setter<usize>,
        get: Getter<usize>,
    },

    Bool {
        get: Getter<bool>,
        set: Setter<bool>,
    },
}

pub struct SettingsDialogTab {
    name: String,
    settings: Vec<(String, SettingsDialogSetting)>,
    current_setting: usize,
}

impl SettingsDialogTab {
    pub fn new(name: String, settings: Vec<(String, SettingsDialogSetting)>) -> Self {
        Self {
            name,
            settings,
            current_setting: 0,
        }
    }

    fn change_setting(&self, steps: i32) {
        let setting = &self.settings[self.current_setting].1;

        match setting {
            SettingsDialogSetting::Float {
                min,
                max,
                mult,
                set,
                get,
            } => set((get() + steps as f32 * mult).clamp(*min, *max)),
            SettingsDialogSetting::Int {
                min,
                max,
                step,
                div: _,
                set,
                get,
            } => set((get() + steps * step).clamp(*min, *max)),
            SettingsDialogSetting::Enum { options, set, get } => {
                set((get() as i32 + steps).rem_euclid(options.len() as i32) as usize)
            }
            SettingsDialogSetting::Bool { get, set } => set(!get()),
        }
    }
}

pub struct SettingsDialog {
    pub show: bool,
    tabs: Vec<SettingsDialogTab>,
    input_state: InputState,
    current_tab: usize,
}

impl SettingsDialog {
    pub fn new(tabs: Vec<SettingsDialogTab>, input_state: InputState) -> Self {
        Self {
            show: false,
            current_tab: 0,
            tabs,
            input_state,
        }
    }

    pub fn on_button_press(&self, button: UscButton) {}
    pub fn on_input(&self, input: UscInputEvent) {}
}

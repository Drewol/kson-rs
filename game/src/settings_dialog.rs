use std::{
    rc::Rc,
    sync::{atomic::AtomicU32, Arc},
};

use kson::Side;
use tealr::mlu::mlua::{Function, Lua, LuaSerdeExt, ToLua};

use crate::{
    button_codes::{UscButton, UscInputEvent},
    config::GameConfig,
    input_state::InputState,
    songselect::KNOB_NAV_THRESHOLD,
};

type Setter<T> = Box<dyn Fn(T) + Send>;
type Getter<T> = Box<dyn Fn() -> T + Send>;

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
    Button {
        action: Getter<()>,
    },
}

pub struct SettingsDialogTab {
    name: String,
    settings: Vec<(String, SettingsDialogSetting)>,
    current_setting: usize,
}

impl<'lua> ToLua<'lua> for &SettingsDialogTab {
    fn to_lua(self, lua: &'lua Lua) -> tealr::mlu::mlua::Result<tealr::mlu::mlua::Value<'lua>> {
        let table = lua.create_table()?;

        table.set("name", lua.create_string(&self.name)?)?;

        let settings_table = lua.create_table()?;

        for (i, (name, setting)) in self.settings.iter().enumerate() {
            let setting_table = lua.create_table()?;
            setting_table.set("name", lua.create_string(name)?)?;

            match setting {
                SettingsDialogSetting::Float {
                    min,
                    max,
                    mult: _,
                    set: _,
                    get,
                } => {
                    setting_table.set("type", "float")?;
                    setting_table.set("value", get())?;
                    setting_table.set("min", *min)?;
                    setting_table.set("max", *max)?;
                }
                SettingsDialogSetting::Int {
                    min,
                    max,
                    step: _,
                    div: _,
                    set: _,
                    get,
                } => {
                    setting_table.set("type", "int")?;
                    setting_table.set("value", get())?;
                    setting_table.set("min", *min)?;
                    setting_table.set("max", *max)?;
                }
                SettingsDialogSetting::Enum {
                    options,
                    set: _,
                    get,
                } => {
                    setting_table.set("type", "enum")?;
                    setting_table.set("value", get())?;
                    settings_table.set("options", lua.to_value(options)?)?;
                }
                SettingsDialogSetting::Bool { get, set: _ } => {
                    setting_table.set("type", "bool")?;
                    setting_table.set("value", get())?;
                }
                SettingsDialogSetting::Button { action: _ } => {
                    setting_table.set("type", "button")?;
                }
            }

            settings_table.set(i + 1, tealr::mlu::mlua::Value::Table(setting_table))?;
        }

        table.set("settings", tealr::mlu::mlua::Value::Table(settings_table))?;

        Ok(tealr::mlu::mlua::Value::Table(table))
    }
}

impl SettingsDialogTab {
    pub fn new(name: impl Into<String>, settings: Vec<(String, SettingsDialogSetting)>) -> Self {
        Self {
            name: name.into(),
            settings,
            current_setting: 0,
        }
    }

    fn change_setting(&self, steps: i32) {
        let setting = &self.settings[self.current_setting].1;

        match setting {
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
            _ => {}
        }
    }
}

pub struct SettingsDialog {
    pub show: bool,
    tabs: Vec<SettingsDialogTab>,
    input_state: InputState,
    current_tab: usize,
    lua: Rc<Lua>,
    setting_advance: f32,
}

impl<'lua> ToLua<'lua> for &SettingsDialog {
    fn to_lua(self, lua: &'lua Lua) -> tealr::mlu::mlua::Result<tealr::mlu::mlua::Value<'lua>> {
        let table = lua.create_table()?;

        table.set("currentTab", self.current_tab + 1)?;
        table.set(
            "currentSetting",
            self.tabs[self.current_tab].current_setting + 1,
        )?;
        let tabs_table = lua.create_table()?;

        for (i, tab) in self.tabs.iter().enumerate() {
            tabs_table.set(i + 1, tab)?;
        }

        table.set("tabs", tealr::mlu::mlua::Value::Table(tabs_table))?;

        Ok(tealr::mlu::mlua::Value::Table(table))
    }
}

impl SettingsDialog {
    pub fn new(tabs: Vec<SettingsDialogTab>, input_state: InputState) -> Self {
        Self {
            show: false,
            current_tab: 0,
            tabs,
            input_state,
            lua: Rc::new(Lua::new()),
            setting_advance: 0.0,
        }
    }

    pub fn on_button_press(&mut self, button: UscButton) {
        match button {
            UscButton::BT(l) => self.tabs[self.current_tab].change_setting(match l {
                kson::BtLane::A => -5,
                kson::BtLane::B => -1,
                kson::BtLane::C => 1,
                kson::BtLane::D => 5,
            }),
            UscButton::FX(s) => {
                let press_time = std::time::SystemTime::now();

                if let Some(other_press_time) =
                    self.input_state.is_button_held(UscButton::FX(s.opposite()))
                {
                    let detla_ms = press_time
                        .duration_since(other_press_time)
                        .unwrap()
                        .as_millis();

                    if detla_ms < 100 {
                        self.show = !self.show;
                    }
                } else {
                    self.current_tab = (self.current_tab as i32
                        + match s {
                            Side::Left => -1,
                            Side::Right => 1,
                        })
                    .rem_euclid(self.tabs.len() as i32)
                        as usize;
                }
            }
            UscButton::Start => {
                let tab = &mut self.tabs[self.current_tab];
                let setting = &mut tab.settings[tab.current_setting];
                if let (_, SettingsDialogSetting::Button { action }) = setting {
                    action();
                }
            }
            UscButton::Back => self.show = false,
            UscButton::Laser(_, _) => {}
            UscButton::Other(_) => {}
        }

        _ = self.lua.globals().set("SettingsDiag", &*self);
    }
    pub fn on_input(&mut self, input: &UscInputEvent) {
        let UscInputEvent::Laser(ls) = input else {
            return;
        };

        self.setting_advance += ls.get_axis(Side::Left).delta;
        let mut value_advance = ls.get_axis(Side::Right).delta / std::f32::consts::PI;

        let settings_steps = (self.setting_advance / KNOB_NAV_THRESHOLD).trunc() as i32;

        self.setting_advance -= settings_steps as f32 * KNOB_NAV_THRESHOLD;

        let tab = &mut self.tabs[self.current_tab];

        tab.current_setting = (tab.current_setting as i32 + settings_steps)
            .rem_euclid(tab.settings.len() as i32) as usize;

        let setting = &mut tab.settings[tab.current_setting];

        let (_, SettingsDialogSetting::Float { min, max, mult: _, set, get }) = setting else {
            _ = self.lua.globals().set("SettingsDiag", &*self);
            return;
        };

        let mut val = get();

        if self
            .input_state
            .is_button_held(UscButton::BT(kson::BtLane::A))
            .is_some()
        {
            value_advance /= 4.0;
        }
        if self
            .input_state
            .is_button_held(UscButton::BT(kson::BtLane::B))
            .is_some()
        {
            value_advance /= 2.0;
        }
        if self
            .input_state
            .is_button_held(UscButton::BT(kson::BtLane::C))
            .is_some()
        {
            value_advance *= 2.0;
        }
        if self
            .input_state
            .is_button_held(UscButton::BT(kson::BtLane::D))
            .is_some()
        {
            value_advance *= 4.0;
        }

        val = val.clamp(*min, *max);
        set(val + value_advance);
        _ = self.lua.globals().set("SettingsDiag", &*self);
    }
    pub fn init_lua(
        &self,
        load_lua: Rc<dyn Fn(Rc<Lua>, &'static str) -> anyhow::Result<generational_arena::Index>>,
    ) -> anyhow::Result<()> {
        self.lua.globals().set("SettingsDiag", self)?;
        load_lua(self.lua.clone(), "gamesettingsdialog.lua")?;
        Ok(())
    }

    pub fn general_settings(input_state: InputState) -> Self {
        let tx = Arc::new(AtomicU32::new(0));
        let rx = tx.clone();

        Self::new(
            vec![
                SettingsDialogTab::new(
                    "Offsets",
                    vec![(
                        "Global Offset".into(),
                        SettingsDialogSetting::Int {
                            min: -100,
                            max: 100,
                            step: 1,
                            div: 1,
                            set: Box::new(|x| GameConfig::get_mut().global_offset = x),
                            get: Box::new(|| GameConfig::get().global_offset),
                        },
                    )],
                ),
                SettingsDialogTab::new(
                    "Test",
                    vec![(
                        "Float Test".into(),
                        SettingsDialogSetting::Float {
                            min: 0.0,
                            max: 1.0,
                            mult: 1.0,
                            set: Box::new(move |v| {
                                tx.store(
                                    (v * std::u32::MAX as f32) as u32,
                                    std::sync::atomic::Ordering::Relaxed,
                                )
                            }),
                            get: Box::new(move || {
                                rx.load(std::sync::atomic::Ordering::Relaxed) as f32
                                    / std::u32::MAX as f32
                            }),
                        },
                    )],
                ),
                SettingsDialogTab::new(
                    "Test",
                    vec![(
                        "Button Test".into(),
                        SettingsDialogSetting::Button {
                            action: Box::new(|| log::info!("SettingsDialog button pressed")),
                        },
                    )],
                ),
            ],
            input_state,
        )
    }

    pub fn render(&mut self, dt: f64) -> anyhow::Result<()> {
        let function: Function = self.lua.globals().get("render")?;
        function.call::<_, ()>((dt / 1000.0, self.show))?;
        Ok(())
    }
}

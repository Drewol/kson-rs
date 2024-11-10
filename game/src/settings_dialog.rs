use std::{
    rc::Rc,
    sync::{
        atomic::{AtomicI32, AtomicU32},
        Arc,
    },
};

use kson::Side;
use log::info;
use mlua::{Function, IntoLua, Lua, LuaSerdeExt};
use std::sync::mpsc::Sender;

use crate::{
    async_service::AsyncService,
    button_codes::{UscButton, UscInputEvent},
    config::{GameConfig, ScoreDisplayMode},
    game::HitWindow,
    game_main::AutoPlay,
    input_state::InputState,
    lua_service::LuaProvider,
    settings_screen::HitFrames,
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

impl SettingsDialogSetting {
    fn button(action: impl Fn() + Send + 'static) -> Self {
        Self::Button {
            action: Box::new(action),
        }
    }

    fn bool(get: impl Fn() -> bool + Send + 'static, set: impl Fn(bool) + Send + 'static) -> Self {
        Self::Bool {
            get: Box::new(get),
            set: Box::new(set),
        }
    }

    fn options(
        get: impl Fn() -> usize + Send + 'static,
        set: impl Fn(usize) + Send + 'static,
        options: Vec<String>,
    ) -> Self {
        Self::Enum {
            options,
            get: Box::new(get),
            set: Box::new(set),
        }
    }

    fn float(
        get: impl Fn() -> f32 + Send + 'static,
        set: impl Fn(f32) + Send + 'static,
        min: f32,
        max: f32,
        mult: f32,
    ) -> Self {
        Self::Float {
            min,
            max,
            mult,
            get: Box::new(get),
            set: Box::new(set),
        }
    }

    fn int(
        get: impl Fn() -> i32 + Send + 'static,
        set: impl Fn(i32) + Send + 'static,
        min: i32,
        max: i32,
        step: i32,
        div: i32,
    ) -> Self {
        Self::Int {
            min,
            max,
            step,
            div,
            get: Box::new(get),
            set: Box::new(set),
        }
    }
}

pub struct SettingsDialogTab {
    name: String,
    settings: Vec<(String, SettingsDialogSetting)>,
    current_setting: usize,
}

impl IntoLua for &SettingsDialogTab {
    fn into_lua(self, lua: &Lua) -> mlua::Result<mlua::Value> {
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
                    setting_table.set("value", get() + 1)?; // lua 1 indexed
                    setting_table.set("options", lua.to_value(options)?)?;
                }
                SettingsDialogSetting::Bool { get, set: _ } => {
                    setting_table.set("type", "bool")?;
                    setting_table.set("value", get())?;
                }
                SettingsDialogSetting::Button { action: _ } => {
                    setting_table.set("type", "button")?;
                }
            }

            settings_table.set(i + 1, mlua::Value::Table(setting_table))?;
        }

        table.set("settings", mlua::Value::Table(settings_table))?;

        Ok(mlua::Value::Table(table))
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
    async_service: di::RefMut<AsyncService>,
}

impl IntoLua for &SettingsDialog {
    fn into_lua(self, lua: &Lua) -> mlua::Result<mlua::Value> {
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

        table.set("tabs", mlua::Value::Table(tabs_table))?;

        Ok(mlua::Value::Table(table))
    }
}

impl SettingsDialog {
    pub fn new(
        tabs: Vec<SettingsDialogTab>,
        input_state: InputState,
        services: di::ServiceProvider,
    ) -> Self {
        Self {
            show: false,
            current_tab: 0,
            tabs,
            input_state,
            lua: LuaProvider::new_lua(),
            setting_advance: 0.0,
            async_service: services.get_required(),
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
                        .expect("Time decreased?")
                        .as_millis();

                    if detla_ms < 100 {
                        if self.show {
                            self.async_service.read().expect("Lock error").save_config();
                        }
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
            UscButton::Refresh => {}
        }

        _ = self.lua.globals().set("SettingsDiag", &*self);
    }
    pub fn on_input(&mut self, input: &UscInputEvent) {
        let UscInputEvent::Laser(ls, _) = input else {
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

        let (
            _,
            SettingsDialogSetting::Float {
                min,
                max,
                mult: _,
                set,
                get,
            },
        ) = setting
        else {
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
    pub fn init_lua(&self, load_lua: &LuaProvider) -> anyhow::Result<()> {
        self.lua.globals().set("SettingsDiag", self)?;
        load_lua.register_libraries(self.lua.clone(), "gamesettingsdialog.lua")?;
        Ok(())
    }

    pub fn general_settings(
        input_state: InputState,
        services: di::ServiceProvider,
        autoplay_tx: Sender<AutoPlay>,
    ) -> Self {
        let tx = Arc::new(AtomicU32::new(0));
        let rx = tx.clone();

        let itx = Arc::new(AtomicI32::new(0));
        let irx = itx.clone();

        Self::new(
            vec![
                SettingsDialogTab::new(
                    "Offsets",
                    vec![
                        (
                            "Global Offset".into(),
                            SettingsDialogSetting::Int {
                                min: -100,
                                max: 100,
                                step: 1,
                                div: 1,
                                set: Box::new(|x| GameConfig::get_mut().global_offset = x),
                                get: Box::new(|| GameConfig::get().global_offset),
                            },
                        ),
                        (
                            "Button Offset".into(),
                            SettingsDialogSetting::int(
                                || GameConfig::get().button_offset,
                                |x| GameConfig::get_mut().button_offset = x,
                                -300,
                                300,
                                1,
                                1,
                            ),
                        ),
                        (
                            "Laser Offset".into(),
                            SettingsDialogSetting::int(
                                || GameConfig::get().laser_offset,
                                |x| GameConfig::get_mut().laser_offset = x,
                                -300,
                                300,
                                1,
                                1,
                            ),
                        ),
                    ],
                ),
                SettingsDialogTab::new(
                    "Game",
                    vec![
                        (
                            "Gauge".into(),
                            SettingsDialogSetting::options(
                                || match GameConfig::get().start_gauge {
                                    crate::game::gauge::GaugeType::Normal => 0,
                                    crate::game::gauge::GaugeType::Hard => 1,
                                },
                                |x| {
                                    GameConfig::get_mut().start_gauge = match x {
                                        1 => crate::game::gauge::GaugeType::Hard,
                                        _ => crate::game::gauge::GaugeType::Normal,
                                    }
                                },
                                vec!["Normal".into(), "Hard".into()],
                            ),
                        ),
                        (
                            "Backup Gauge".into(),
                            SettingsDialogSetting::bool(
                                || GameConfig::get().fallback_gauge,
                                |x| GameConfig::get_mut().fallback_gauge = x,
                            ),
                        ),
                        (
                            "Hide Background".into(),
                            SettingsDialogSetting::bool(
                                || GameConfig::get().graphics.disable_bg,
                                |x| GameConfig::get_mut().graphics.disable_bg = x,
                            ),
                        ),
                        (
                            "Score Display".into(),
                            SettingsDialogSetting::options(
                                || match GameConfig::get().score_display {
                                    ScoreDisplayMode::Additive => 0,
                                    ScoreDisplayMode::Subtractive => 1,
                                    ScoreDisplayMode::Average => 2,
                                },
                                |x| {
                                    GameConfig::get_mut().score_display = match x {
                                        0 => ScoreDisplayMode::Additive,
                                        1 => ScoreDisplayMode::Subtractive,
                                        2 => ScoreDisplayMode::Average,
                                        _ => ScoreDisplayMode::default(),
                                    }
                                },
                                vec![
                                    ScoreDisplayMode::Additive.to_string(),
                                    ScoreDisplayMode::Subtractive.to_string(),
                                    ScoreDisplayMode::Average.to_string(),
                                ],
                            ),
                        ),
                        (
                            "Autoplay".into(),
                            SettingsDialogSetting::button(move || {
                                autoplay_tx.send(AutoPlay::All).unwrap()
                            }),
                        ),
                    ],
                ),
                SettingsDialogTab::new(
                    "Judgement",
                    vec![
                        (
                            "Crit window".into(),
                            SettingsDialogSetting::int(
                                || {
                                    HitFrames::from(GameConfig::get().hit_window.perfect)
                                        .0
                                        .round() as i32
                                },
                                |x| {
                                    GameConfig::get_mut().hit_window.perfect =
                                        HitFrames(x as _).into()
                                },
                                1,
                                20,
                                1,
                                1,
                            ),
                        ),
                        (
                            "Near window".into(),
                            SettingsDialogSetting::int(
                                || {
                                    HitFrames::from(GameConfig::get().hit_window.good).0.round()
                                        as i32
                                },
                                |x| {
                                    GameConfig::get_mut().hit_window.good = HitFrames(x as _).into()
                                },
                                1,
                                20,
                                1,
                                1,
                            ),
                        ),
                        (
                            "Crit window".into(),
                            SettingsDialogSetting::int(
                                || {
                                    HitFrames::from(GameConfig::get().hit_window.hold).0.round()
                                        as i32
                                },
                                |x| {
                                    GameConfig::get_mut().hit_window.hold = HitFrames(x as _).into()
                                },
                                1,
                                20,
                                1,
                                1,
                            ),
                        ),
                        (
                            "Set Normal".into(),
                            SettingsDialogSetting::button(|| {
                                GameConfig::get_mut().hit_window = HitWindow::NORMAL
                            }),
                        ),
                        (
                            "Set Hard".into(),
                            SettingsDialogSetting::button(|| {
                                GameConfig::get_mut().hit_window = HitWindow::HARD
                            }),
                        ),
                    ],
                ),
                SettingsDialogTab::new(
                    "Test",
                    vec![
                        (
                            "Float Test".into(),
                            SettingsDialogSetting::float(
                                move || {
                                    rx.load(std::sync::atomic::Ordering::Relaxed) as f32
                                        / u32::MAX as f32
                                },
                                move |v| {
                                    tx.store(
                                        (v * u32::MAX as f32) as u32,
                                        std::sync::atomic::Ordering::Relaxed,
                                    )
                                },
                                0.0,
                                1.0,
                                1.0,
                            ),
                        ),
                        (
                            "Int Test".into(),
                            SettingsDialogSetting::int(
                                move || irx.load(std::sync::atomic::Ordering::Relaxed),
                                move |x| itx.store(x, std::sync::atomic::Ordering::Relaxed),
                                -100,
                                100,
                                5,
                                1,
                            ),
                        ),
                        (
                            "Button Test".into(),
                            SettingsDialogSetting::button(|| info!("Test button pressed")),
                        ),
                    ],
                ),
            ],
            input_state,
            services,
        )
    }

    pub fn render(&mut self, dt: f64) -> anyhow::Result<()> {
        let function: Function = self.lua.globals().get("render")?;
        function.call::<()>((dt / 1000.0, self.show))?;
        Ok(())
    }
}

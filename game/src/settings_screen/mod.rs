mod controller_binding;
pub mod skin_select;

use std::{collections::HashMap, path::PathBuf, sync::mpsc::Sender, time::Duration};

use di::ServiceProvider;
use egui::{CollapsingResponse, InnerResponse, RichText, Separator, Slider, TextEdit, Ui};
use gilrs::GamepadId;
use itertools::Itertools;
use skin_select::SkinMeta;
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    monitor::MonitorHandle,
};

use crate::{
    config::{Fullscreen, GameConfig, ScoreDisplayMode},
    game::HitWindow,
    game_main::ControlMessage,
    input_state::InputState,
    scene::Scene,
    skin_settings::SkinSettingValue,
};

use self::controller_binding::BindingUi;

pub struct SettingsScreen {
    altered_settings: GameConfig,
    close: bool,
    input_state: InputState,
    selected_controller: Option<GamepadId>,
    binding_ui: Option<BindingUi>,
    controllers: HashMap<GamepadId, String>,
    monitors: Vec<MonitorHandle>,
    primary_monitor: Option<MonitorHandle>,
    tx: Sender<ControlMessage>,
    skins: Vec<(SkinMeta, PathBuf)>,
}

impl SettingsScreen {
    pub fn new(
        services: ServiceProvider,
        tx: Sender<ControlMessage>,
        window: &winit::window::Window,
    ) -> Self {
        let input_state = InputState::clone(&services.get_required());
        let controllers = {
            let lock_gilrs = input_state.lock_gilrs();
            lock_gilrs
                .gamepads()
                .map(|(id, pad)| (id, pad.name().to_string()))
                .collect()
        };

        let monitors = window.available_monitors().collect_vec();
        let primary_monitor = window.current_monitor();

        let mut skins_folder = crate::default_game_dir();
        skins_folder.push("skins");
        let skins = skins_folder
            .read_dir()
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|x| x.ok())
            .filter(|x| x.file_type().is_ok_and(|x| x.is_dir()))
            .map(|x| x.path())
            .map(|mut p| {
                p.push("meta.json");
                if let Ok(Ok(m)) = std::fs::File::open(&p).map(serde_json::from_reader) {
                    p.pop();
                    (m, p)
                } else {
                    p.pop();
                    (
                        SkinMeta::named(p.file_name().and_then(|x| x.to_str()).unwrap_or("unk")),
                        p,
                    )
                }
            })
            .collect();

        Self {
            altered_settings: GameConfig::get().clone(),
            close: false,
            input_state,
            selected_controller: None,
            binding_ui: None,
            controllers,
            monitors,
            primary_monitor,
            tx,
            skins,
        }
    }

    fn apply(&self) {
        let mut c = GameConfig::get_mut();
        *c = self.altered_settings.clone();
        _ = self.tx.send(ControlMessage::ApplySettings);
    }
}

pub struct HitFrames(pub f64);

impl From<HitFrames> for Duration {
    fn from(val: HitFrames) -> Self {
        Duration::from_secs_f64(val.0 / 120.0)
    }
}
impl From<Duration> for HitFrames {
    fn from(value: Duration) -> Self {
        Self(120.0 * value.as_secs_f64())
    }
}

impl Scene for SettingsScreen {
    fn render_ui(&mut self, _dt: f64) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_suspended(&self) -> bool {
        false
    }

    fn debug_ui(&mut self, _ctx: &egui::Context) -> anyhow::Result<()> {
        Ok(())
    }

    fn closed(&self) -> bool {
        self.close
    }

    fn name(&self) -> &str {
        "Settings"
    }

    fn tick(
        &mut self,
        _dt: f64,
        _knob_state: crate::button_codes::LaserState,
    ) -> anyhow::Result<()> {
        if let Some(binding_ui) = self.binding_ui.as_mut() {
            binding_ui.run_checks(&mut self.altered_settings)
        }

        Ok(())
    }

    fn has_egui(&self) -> bool {
        true
    }

    fn render_egui(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        egui::panel::TopBottomPanel::bottom("settings_buttons").show(ctx, |ui| {
            if ui.button("Cancel").clicked() {
                self.close = true;
            }

            if ui.button("Apply").clicked() {
                self.apply();
                self.close = true;
            }
        });

        egui::panel::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                settings_section("Input", ui, |ui| {
                    ui.label("Offset");
                    ui.add(Slider::new(
                        &mut self.altered_settings.global_offset,
                        -100..=100,
                    ));
                    ui.end_row();
                    ui.checkbox(
                        &mut self.altered_settings.keyboard_buttons,
                        "Keyboard buttons",
                    );
                    ui.end_row();
                    ui.checkbox(&mut self.altered_settings.keyboard_knobs, "Keyboard knobs");
                    ui.end_row();
                    ui.checkbox(&mut self.altered_settings.mouse_knobs, "Mouse knobs");
                    ui.end_row();

                    egui::ComboBox::from_label("Controller")
                        .selected_text(
                            self.selected_controller
                                .and_then(|id| self.controllers.get(&id))
                                .unwrap_or(&"None".to_string()),
                        )
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_value(&mut self.selected_controller, None, "None")
                                .clicked()
                            {
                                self.binding_ui = None;
                            }

                            for (id, name) in self.controllers.iter() {
                                if ui
                                    .selectable_value(
                                        &mut self.selected_controller,
                                        Some(*id),
                                        name,
                                    )
                                    .clicked()
                                {
                                    self.binding_ui =
                                        Some(BindingUi::new(*id, self.input_state.clone()));
                                }
                            }
                        });
                    ui.end_row();
                    if let Some(binding_ui) = self.binding_ui.as_mut() {
                        binding_ui.ui(ui, &mut self.altered_settings);
                    }
                });

                settings_section("Game", ui, |ui| {
                    let mut crit_frames: HitFrames =
                        self.altered_settings.hit_window.perfect.into();
                    let mut near_frames: HitFrames = self.altered_settings.hit_window.good.into();
                    let mut hold_frames: HitFrames = self.altered_settings.hit_window.hold.into();

                    ui.label("Hit windows (in frames @ 60fps)");
                    ui.end_row();
                    egui::Grid::new("hit_windows")
                        .num_columns(3)
                        .show(ui, |ui| {
                            ui.label("Crit");
                            ui.label("Near");
                            ui.label("Hold");
                            ui.end_row();

                            if ui
                                .add(
                                    egui::DragValue::new(&mut crit_frames.0)
                                        .max_decimals(1)
                                        .clamp_range(0.01..=100.0),
                                )
                                .changed()
                            {
                                self.altered_settings.hit_window.perfect = crit_frames.into();
                            }

                            if ui
                                .add(
                                    egui::DragValue::new(&mut near_frames.0)
                                        .max_decimals(1)
                                        .clamp_range(0.01..=100.0),
                                )
                                .changed()
                            {
                                self.altered_settings.hit_window.good = near_frames.into();
                            }

                            if ui
                                .add(
                                    egui::DragValue::new(&mut hold_frames.0)
                                        .max_decimals(1)
                                        .clamp_range(0.01..=100.0),
                                )
                                .changed()
                            {
                                self.altered_settings.hit_window.hold = hold_frames.into();
                            }
                        });
                    ui.end_row();
                    if ui.button("Set Normal").clicked() {
                        self.altered_settings.hit_window = HitWindow::NORMAL;
                    }
                    if ui.button("Set Hard").clicked() {
                        self.altered_settings.hit_window = HitWindow::HARD;
                    }

                    ui.end_row();
                    egui::ComboBox::new("score_display_mode", "Score display mode")
                        .selected_text(self.altered_settings.score_display.to_string())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.altered_settings.score_display,
                                ScoreDisplayMode::Additive,
                                ScoreDisplayMode::Additive.to_string(),
                            );
                            ui.selectable_value(
                                &mut self.altered_settings.score_display,
                                ScoreDisplayMode::Subtractive,
                                ScoreDisplayMode::Subtractive.to_string(),
                            );
                            ui.selectable_value(
                                &mut self.altered_settings.score_display,
                                ScoreDisplayMode::Average,
                                ScoreDisplayMode::Average.to_string(),
                            );
                        })
                });

                settings_section("Graphics", ui, |ui| {
                    ui.checkbox(&mut self.altered_settings.graphics.vsync, "VSync");
                    ui.end_row();
                    ui.checkbox(&mut self.altered_settings.graphics.show_fps, "Show FPS");
                    ui.end_row();
                    ui.checkbox(
                        &mut self.altered_settings.graphics.disable_bg,
                        "Disable Backgrounds",
                    );
                    ui.end_row();
                    egui::ComboBox::from_label("Anti Aliasing")
                        .selected_text(aa_text(self.altered_settings.graphics.anti_alias))
                        .show_ui(ui, |ui| {
                            for i in 0..4 {
                                let aa = 1 << i;
                                if ui
                                    .selectable_label(
                                        aa == self.altered_settings.graphics.anti_alias,
                                        aa_text(aa),
                                    )
                                    .clicked()
                                {
                                    self.altered_settings.graphics.anti_alias = aa;
                                }
                            }
                        });
                    ui.end_row();
                    let window_mode = match self.altered_settings.graphics.fullscreen {
                        crate::config::Fullscreen::Windowed { .. } => 0,
                        crate::config::Fullscreen::Borderless { .. } => 1,
                        crate::config::Fullscreen::Exclusive { .. } => 2,
                    };
                    egui::ComboBox::from_label("Window mode")
                        .selected_text(match window_mode {
                            0 => "Windowed",
                            1 => "Borderless Fullscreen",
                            2 => "Exclusive Fullscreen",
                            _ => unreachable!(),
                        })
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(window_mode == 0, "Windowed").clicked()
                                && window_mode != 0
                            {
                                self.altered_settings.graphics.fullscreen = Fullscreen::Windowed {
                                    pos: self
                                        .primary_monitor
                                        .as_ref()
                                        .map(|x| x.position())
                                        .unwrap_or(PhysicalPosition::new(0, 0)),
                                    size: PhysicalSize::new(1280, 720),
                                };
                            }

                            if ui
                                .selectable_label(window_mode == 1, "Borderless Fullscreen")
                                .clicked()
                                && window_mode != 1
                            {
                                self.altered_settings.graphics.fullscreen = Fullscreen::Borderless {
                                    monitor: self
                                        .primary_monitor
                                        .as_ref()
                                        .map(|x| x.position())
                                        .unwrap_or(PhysicalPosition::new(0, 0)),
                                }
                            }
                            if ui
                                .selectable_label(window_mode == 2, "Exclusive Fullscreen")
                                .clicked()
                                && window_mode != 2
                            {
                                self.altered_settings.graphics.fullscreen = Fullscreen::Exclusive {
                                    resolution: self
                                        .primary_monitor
                                        .as_ref()
                                        .map(|x| x.size())
                                        .unwrap_or(PhysicalSize::new(1280, 720)),
                                    monitor: self
                                        .primary_monitor
                                        .as_ref()
                                        .map(|x| x.position())
                                        .unwrap_or(PhysicalPosition::new(0, 0)),
                                }
                            }
                        });
                    ui.end_row();
                    match &mut self.altered_settings.graphics.fullscreen {
                        Fullscreen::Windowed { .. } => {}
                        Fullscreen::Borderless { monitor } => {
                            monitor_select(monitor, ui, &self.monitors);
                        }
                        Fullscreen::Exclusive {
                            monitor,
                            resolution,
                        } => {
                            monitor_select(monitor, ui, &self.monitors);
                            ui.end_row();
                            if let Some(monitor) =
                                self.monitors.iter().find(|x| x.position() == *monitor)
                            {
                                egui::ComboBox::from_label("Resolution")
                                    .selected_text(format!(
                                        "{}x{}",
                                        resolution.width, resolution.height
                                    ))
                                    .show_ui(ui, |ui| {
                                        for mode in monitor.video_modes().unique_by(|x| x.size()) {
                                            let mode_resolution = mode.size();
                                            if ui
                                                .selectable_label(
                                                    *resolution == mode_resolution,
                                                    format!(
                                                        "{}x{}",
                                                        mode_resolution.width,
                                                        mode_resolution.height
                                                    ),
                                                )
                                                .clicked()
                                            {
                                                *resolution = mode_resolution;
                                            }
                                        }
                                    });
                            }
                        }
                    }
                    ui.end_row();
                    ui.label("Distant button scale");
                    let slider_width = ui
                        .add(
                            egui::Slider::new(
                                &mut self.altered_settings.distant_button_scale,
                                1.0..=5.0,
                            )
                            .logarithmic(true),
                        )
                        .rect
                        .width();
                    let (color_a, color_b) = self
                        .altered_settings
                        .laser_hues
                        .iter()
                        .copied()
                        .map(|x| egui::epaint::Hsva::new(x / 360.0, 1.0, 1.0, 1.0))
                        .collect_tuple()
                        .expect("Invalid number of laser hues");
                    ui.end_row();
                    ui.label("Laser colors");
                    ui.end_row();
                    egui::color_picker::show_color(ui, color_a, egui::vec2(slider_width, 20.0));
                    egui::color_picker::show_color(ui, color_b, egui::vec2(slider_width, 20.0));
                    ui.end_row();
                    for hue in self.altered_settings.laser_hues.iter_mut() {
                        ui.add(egui::Slider::new(hue, 0.0..=360.0)).rect.width();
                    }
                    ui.end_row();
                    if ui.button("Reset hues").clicked() {
                        self.altered_settings.laser_hues = [200.0, 330.0];
                    }
                });

                settings_section("Audio", ui, |ui| {
                    ui.label("Master avolume");
                    ui.add(
                        Slider::new(&mut self.altered_settings.master_volume, 0.0..=1.0)
                            .custom_formatter(|x, _| format!("{:.0}%", x * 100.0))
                            .custom_parser(|x| x.trim_matches('%').trim().parse().ok()),
                    )
                });

                settings_section("Skin", ui, |ui| {
                    let current_skin = self
                        .skins
                        .iter()
                        .find(|x| x.1.ends_with(&self.altered_settings.skin))
                        .map(|x| x.0.name.clone())
                        .unwrap_or_default();

                    egui::ComboBox::new("skin_select", "Selected skin")
                        .selected_text(&current_skin)
                        .show_ui(ui, |ui| {
                            for (meta, path) in self.skins.iter() {
                                if ui
                                    .selectable_label(path.ends_with(&current_skin), &meta.name)
                                    .clicked()
                                {
                                    if let Some(v) = path
                                        .file_name()
                                        .and_then(|x| x.to_str())
                                        .map(|x| x.to_string())
                                    {
                                        self.altered_settings.skin = v;
                                    }
                                }
                            }
                        });

                    ui.end_row();
                    ui.separator();
                    ui.end_row();

                    for ele in &self.altered_settings.skin_definition {
                        match ele {
                            crate::skin_settings::SkinSettingEntry::Label { v } => {
                                ui.heading(v);
                            }
                            crate::skin_settings::SkinSettingEntry::Separator => {
                                ui.add(Separator::default().grow(0.0).spacing(5.0).horizontal());
                            }
                            crate::skin_settings::SkinSettingEntry::Selection {
                                default: _,
                                label,
                                name,
                                values,
                            } => {
                                let Some(SkinSettingValue::Text(t)) =
                                    self.altered_settings.skin_settings.get_mut(name)
                                else {
                                    continue;
                                };
                                egui::containers::ComboBox::from_label(label)
                                    .selected_text(t.clone())
                                    .show_ui(ui, |ui| {
                                        for ele in values {
                                            ui.selectable_value(t, ele.clone(), ele);
                                        }
                                    });
                            }
                            crate::skin_settings::SkinSettingEntry::Text {
                                default: _,
                                label,
                                name,
                                secret,
                            } => {
                                let Some(SkinSettingValue::Text(t)) =
                                    self.altered_settings.skin_settings.get_mut(name)
                                else {
                                    continue;
                                };
                                ui.label(label);
                                ui.add(TextEdit::singleline(t).password(*secret));
                            }
                            crate::skin_settings::SkinSettingEntry::Color {
                                default: _,
                                label,
                                name,
                            } => {
                                let Some(SkinSettingValue::Color(col)) =
                                    self.altered_settings.skin_settings.get_mut(name)
                                else {
                                    continue;
                                };
                                ui.label(label);
                                ui.color_edit_button_srgba(&mut col.0);
                            }
                            crate::skin_settings::SkinSettingEntry::Bool {
                                default: _,
                                label,
                                name,
                            } => {
                                let Some(SkinSettingValue::Bool(v)) =
                                    self.altered_settings.skin_settings.get_mut(name)
                                else {
                                    continue;
                                };
                                ui.checkbox(v, label);
                            }
                            crate::skin_settings::SkinSettingEntry::Float {
                                default: _,
                                label,
                                name,
                                min,
                                max,
                            } => {
                                let Some(SkinSettingValue::Float(v)) =
                                    self.altered_settings.skin_settings.get_mut(name)
                                else {
                                    continue;
                                };
                                ui.label(label);
                                ui.add(egui::Slider::new(v, *min..=*max));
                            }
                            crate::skin_settings::SkinSettingEntry::Integer {
                                default: _,
                                label,
                                name,
                                min,
                                max,
                            } => {
                                let Some(SkinSettingValue::Integer(v)) =
                                    self.altered_settings.skin_settings.get_mut(name)
                                else {
                                    continue;
                                };
                                ui.label(label);
                                ui.add(egui::Slider::new(v, *min..=*max));
                            }
                        }
                        ui.end_row();
                    }
                });
            });
        });

        Ok(())
    }
}

fn monitor_select(
    selected_monitor: &mut PhysicalPosition<i32>,
    ui: &mut Ui,
    monitors: &[MonitorHandle],
) {
    let Some(default_monitor) = monitors.first() else {
        log::warn!("Could not iterate monitors");
        return;
    };

    let (current_index, current_monitor) = monitors
        .iter()
        .cloned()
        .enumerate()
        .find(|x| x.1.position() == *selected_monitor)
        .unwrap_or((0, default_monitor.clone()));

    egui::ComboBox::from_label("Monitor")
        .selected_text(
            current_monitor
                .name()
                .unwrap_or_else(|| current_index.to_string()),
        )
        .show_ui(ui, |ui| {
            for (index, monitor) in monitors.iter().enumerate() {
                if ui
                    .selectable_label(
                        index == current_index,
                        monitor.name().unwrap_or_else(|| index.to_string()),
                    )
                    .clicked()
                {
                    *selected_monitor = monitor.position();
                }
            }
        });
}

fn aa_text(aa: u8) -> String {
    match aa {
        1 => "Off".into(),
        v => format!("{v}x"),
    }
}

fn settings_section<T>(
    name: &str,
    ui: &mut Ui,
    add_contents: impl FnOnce(&mut Ui) -> T,
) -> CollapsingResponse<InnerResponse<T>> {
    ui.collapsing(RichText::new(name).heading(), |ui| {
        ui.horizontal_wrapped(add_contents)
    })
}

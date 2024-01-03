mod controller_binding;

use std::collections::HashMap;

use egui::{CollapsingResponse, InnerResponse, RichText, Separator, Slider, TextEdit, Ui};
use gilrs::GamepadId;
use itertools::Itertools;

use crate::{
    config::GameConfig, input_state::InputState, scene::Scene, skin_settings::SkinSettingValue,
};

use self::controller_binding::BindingUi;

pub struct SettingsScreen {
    altered_settings: GameConfig,
    close: bool,
    input_state: InputState,
    selected_controller: Option<GamepadId>,
    binding_ui: Option<BindingUi>,
    controllers: HashMap<GamepadId, String>,
}

impl SettingsScreen {
    pub fn new(input_state: InputState) -> Self {
        let controllers = {
            let lock_gilrs = input_state.lock_gilrs();
            lock_gilrs
                .gamepads()
                .map(|(id, pad)| (id, pad.name().to_string()))
                .collect()
        };

        Self {
            altered_settings: GameConfig::get().clone(),
            close: false,
            input_state,
            selected_controller: None,
            binding_ui: None,
            controllers,
        }
    }

    fn apply(&self) {
        let mut c = GameConfig::get_mut();
        *c = self.altered_settings.clone();
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

    fn tick(&mut self, dt: f64, knob_state: crate::button_codes::LaserState) -> anyhow::Result<()> {
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

                settings_section("Skin", ui, |ui| {
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
                                let SkinSettingValue::Text(t) =
                                    self.altered_settings.skin_settings.get_mut(name).unwrap()
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
                                let SkinSettingValue::Text(t) =
                                    self.altered_settings.skin_settings.get_mut(name).unwrap()
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
                                let SkinSettingValue::Color(col) =
                                    self.altered_settings.skin_settings.get_mut(name).unwrap()
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
                                let SkinSettingValue::Bool(v) =
                                    self.altered_settings.skin_settings.get_mut(name).unwrap()
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
                                let SkinSettingValue::Float(v) =
                                    self.altered_settings.skin_settings.get_mut(name).unwrap()
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
                                let SkinSettingValue::Integer(v) =
                                    self.altered_settings.skin_settings.get_mut(name).unwrap()
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

fn settings_section<T>(
    name: &str,
    ui: &mut Ui,
    add_contents: impl FnOnce(&mut Ui) -> T,
) -> CollapsingResponse<InnerResponse<T>> {
    ui.collapsing(RichText::new(name).heading(), |ui| {
        ui.horizontal_wrapped(add_contents)
    })
}

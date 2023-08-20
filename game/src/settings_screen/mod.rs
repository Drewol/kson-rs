use egui::{CollapsingResponse, InnerResponse, RichText, Separator, Slider, TextEdit, Ui};

use crate::{config::GameConfig, scene::Scene, skin_settings::SkinSettingValue};

pub struct SettingsScreen {
    altered_settings: GameConfig,
    close: bool,
}

impl SettingsScreen {
    pub fn new() -> Self {
        Self {
            altered_settings: GameConfig::get().clone(),
            close: false,
        }
    }

    fn apply(&self) {
        let mut c = GameConfig::get_mut();
        *c = self.altered_settings.clone();
    }
}

impl Scene for SettingsScreen {
    fn render_ui(&mut self, dt: f64) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_suspended(&self) -> bool {
        false
    }

    fn debug_ui(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        Ok(())
    }

    fn closed(&self) -> bool {
        self.close
    }

    fn name(&self) -> &str {
        "Settings"
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
                                default,
                                label,
                                name,
                                values,
                            } => {
                                let SkinSettingValue::Text(t) = self.altered_settings.skin_settings.get_mut(name).unwrap() else {continue;};
                                egui::containers::ComboBox::from_label(label)
                                    .selected_text(t.clone())
                                    .show_ui(ui, |ui| {
                                        for ele in values {
                                            ui.selectable_value(t, ele.clone(), ele);
                                        }
                                    });
                            }
                            crate::skin_settings::SkinSettingEntry::Text {
                                default,
                                label,
                                name,
                                secret,
                            } => {
                                let SkinSettingValue::Text(t) = self.altered_settings.skin_settings.get_mut(name).unwrap() else {continue;};
                                ui.label(label);
                                ui.add(TextEdit::singleline(t).password(*secret));
                            }
                            crate::skin_settings::SkinSettingEntry::Color {
                                default,
                                label,
                                name,
                            } => {
                                let  SkinSettingValue::Color(col) =  self.altered_settings.skin_settings.get_mut(name).unwrap() else {continue;};
                                ui.label(label);
                                ui.color_edit_button_srgba(&mut col.0);
                            }
                            crate::skin_settings::SkinSettingEntry::Bool {
                                default,
                                label,
                                name,
                            } => {
                                let  SkinSettingValue::Bool(v) =  self.altered_settings.skin_settings.get_mut(name).unwrap() else {continue;};
                                ui.checkbox(v, label);
                            }
                            crate::skin_settings::SkinSettingEntry::Float {
                                default,
                                label,
                                name,
                                min,
                                max,
                            } => {
                                let  SkinSettingValue::Float(v) =  self.altered_settings.skin_settings.get_mut(name).unwrap() else {continue;};
                                ui.label(label);
                                ui.add(egui::Slider::new(v, *min..=*max));
                            }
                            crate::skin_settings::SkinSettingEntry::Integer {
                                default,
                                label,
                                name,
                                min,
                                max,
                            } => {
                                let  SkinSettingValue::Integer(v) =  self.altered_settings.skin_settings.get_mut(name).unwrap() else {continue;};
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

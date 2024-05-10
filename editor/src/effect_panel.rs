use anyhow::bail;
use eframe::egui::{self, ComboBox};

use crate::{
    chart_editor::MainState,
    effect_editor::EffectEditor,
    i18n::{self, fl},
};

const EFFECT_OPTIONS: [&str; 11] = [
    "Retrigger",
    "Gate",
    "Flanger",
    "PitchShift",
    "BitCrusher",
    "Phaser",
    "Wobble",
    "TapeStop",
    "Echo",
    "SideChain",
    "SwitchAudio",
];

pub fn effect_panel(state: &mut MainState) -> impl egui::Widget + '_ {
    move |ui: &mut egui::Ui| {
        ui.heading(i18n::fl!("effect_definitions"));

        let mut effects = state.chart.audio.audio_effect.take().unwrap_or_default();

        let mut keys: Vec<_> = effects.fx.def.iter_mut().collect();
        keys.sort_by_key(|x| x.0);

        for (key, effect) in keys {
            let unaltered = effect.clone();

            ui.collapsing(key, |ui| {
                effect.edit(ui);
            });

            if unaltered.ne(effect) {
                let action = state.actions.new_action();
                action.description = fl!("alter_effect", name = key.clone());
                let key = key.clone();
                let effect = effect.clone();
                action.action = Box::new(move |c| {
                    let Some(effects) = c.audio.audio_effect.as_mut() else {
                        bail!("No effects defined")
                    };

                    let Some(original) = effects.fx.def.get_mut(&key) else {
                        bail!("Effect not defined")
                    };
                    *original = effect.clone();

                    Ok(())
                })
            };
        }

        let id = ui.next_auto_id();
        //TODO: New effect ui
        let (mut new_name, mut effect_type) = ui
            .data_mut(|x| x.remove_temp::<(String, String)>(id))
            .unwrap_or_default();

        ui.label(fl!("new"));
        ui.end_row();
        ui.label(fl!("name"));
        ui.text_edit_singleline(&mut new_name);
        ComboBox::new("new_effect_type", "")
            .selected_text(&effect_type)
            .show_ui(ui, |ui| {
                for e in EFFECT_OPTIONS {
                    ui.selectable_value(&mut effect_type, e.to_string(), e);
                }
            });

        if ui.button(fl!("new")).clicked()
            && !new_name.is_empty()
            && !effects.fx.def.contains_key(&new_name)
        {
            if let Ok(effect) = kson::effects::AudioEffect::try_from(effect_type.as_str()) {
                let new_action = state.actions.new_action();
                new_action.description = fl!("new_effect_definition", name = new_name.clone());

                new_action.action = Box::new(move |c| {
                    let mut effects = c.audio.audio_effect.take().unwrap_or_default();
                    effects.fx.def.insert(new_name.clone(), effect.clone());
                    c.audio.audio_effect = Some(effects);
                    Ok(())
                });
            } else {
                ui.data_mut(|x| x.insert_temp(id, (new_name, effect_type)));
            }
        } else {
            ui.data_mut(|x| x.insert_temp(id, (new_name, effect_type)));
        }
        state.chart.audio.audio_effect = Some(effects);
        ui.separator()
    }
}

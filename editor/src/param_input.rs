use std::str::FromStr;

use eframe::egui::{self, Widget};
use kson::parameter::EffectParameter;

type GetSetValue<'a, T> = Box<dyn 'a + FnMut(Option<EffectParameter<T>>) -> EffectParameter<T>>;

pub struct ParamEditor<'a, T> {
    get_set_value: GetSetValue<'a, T>,
}

impl<'a, T: Clone> ParamEditor<'a, T> {
    pub fn new(value: &'a mut EffectParameter<T>, _allow_filename: bool) -> Self {
        Self {
            get_set_value: Box::new(move |v: Option<EffectParameter<T>>| {
                //TOOO: Check for filename
                if let Some(v) = v {
                    *value = v;
                }
                value.clone()
            }),
        }
    }
}

#[allow(unused)]
fn is_filename<T>(v: &EffectParameter<T>) -> bool {
    matches!(
        (&v.off, &v.on),
        (kson::parameter::EffectParameterValue::Filename(_), _)
            | (_, Some(kson::parameter::EffectParameterValue::Filename(_)))
    )
}

impl<T: Default + 'static> Widget for ParamEditor<'_, T> {
    fn ui(self, ui: &mut eframe::egui::Ui) -> eframe::egui::Response {
        let Self { mut get_set_value } = self;

        let id = ui.next_auto_id();

        let old_value = get_set_value(None);
        let mut value_text = ui
            .data_mut(|x| x.remove_temp::<String>(id))
            .unwrap_or_else(|| old_value.to_string());
        let response = ui.text_edit_singleline(&mut value_text);

        ui.data_mut(|d| d.insert_temp(id, value_text));

        if response.lost_focus() {
            if let Some(value) = ui.data_mut(|d| d.remove_temp::<String>(id)) {
                get_set_value(EffectParameter::<T>::from_str(&value).ok());
            }
        }

        if !response.has_focus() {
            ui.data_mut(|d| d.insert_temp(id, old_value.to_string()));
        }

        response
    }
}

pub fn param_editor<T: Clone + Default + 'static>(
    param: &mut EffectParameter<T>,
    allow_filename: bool,
) -> impl egui::Widget + '_ {
    let editor = ParamEditor::new(param, allow_filename);
    move |ui: &mut egui::Ui| ui.add(editor)
}

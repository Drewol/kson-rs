use std::{collections::HashMap, ffi::CString, time::SystemTime};

use egui::Ui;
use hidlights::{DeviceInfo, Report};
use itertools::Itertools;
use log::warn;

use crate::lighting::{LightingTarget, MappedTarget};

#[derive(Default)]
pub struct LightingConfig {
    devices: Vec<DeviceInfo>,
    highlighted_device: Option<(hidlights::DeviceHandle, CString)>,
    binding_device: Option<(hidlights::DeviceHandle, Vec<Report>, CString)>,
    highlighted_output: (u32, u32),
}

impl LightingConfig {
    pub fn new() -> Self {
        let Ok(api) = hidlights::HidLights::new() else {
            warn!("Failed to get hidlight api");
            return Self::default();
        };
        let mut devices = api.devices();
        devices.dedup_by(|a, b| a.path() == b.path());
        devices.retain(|x| {
            x.open()
                .is_ok_and(|d| d.reports().is_ok_and(|x| !x.is_empty()))
        });

        Self {
            devices,
            highlighted_device: None,
            binding_device: None,
            highlighted_output: (u32::MAX, u32::MAX),
        }
    }

    fn device_selection(&mut self, ui: &mut Ui) {
        for dev in self.devices.iter() {
            let n = [
                dev.name.as_ref(),
                dev.manufacturer.as_ref(),
                dev.usage.as_ref().map(|x| x.name()).as_ref(),
            ]
            .into_iter()
            .filter_map(|x| x.cloned())
            .collect_vec()
            .join(" / ");

            if ui.button(n).clicked() {
                let device = if let Some((dev, p)) = self
                    .highlighted_device
                    .take()
                    .filter(|x| x.1.as_c_str() == dev.path())
                {
                    Some(dev) //Reuse opened device
                } else {
                    dev.open().ok()
                };

                let p = dev.path().to_owned();
                if let Some(dev) = device {
                    let reports = dev.reports().unwrap_or_default();
                    self.binding_device = Some((dev, reports, p))
                }
            }

            let mut highlighted = self
                .highlighted_device
                .as_ref()
                .is_some_and(|d| d.1.as_c_str() == dev.path());
            if ui.toggle_value(&mut highlighted, "Highlight").changed() {
                if highlighted {
                    let p = dev.path().to_owned();
                    if let Some(dev) = dev.open().ok() {
                        self.highlighted_device = Some((dev, p))
                    }
                } else {
                    self.highlighted_device = None
                }
            }

            ui.end_row();
        }
    }

    pub fn render<'a>(
        &'a mut self,
        config: &'a mut crate::lighting::LightingMap,
    ) -> impl FnOnce(&mut egui::Ui) -> () + 'a {
        self.blink_highlighted();

        |ui| {
            if self.binding_device.is_some() {
                self.output_binding(config, ui);
            } else {
                self.device_selection(ui);
            }
        }
    }

    fn output_binding(&mut self, config: &mut HashMap<String, Vec<MappedTarget>>, ui: &mut Ui) {
        let Some((dev, reports, path)) = self.binding_device.as_mut() else {
            return;
        };
        let config = config
            .entry(path.to_string_lossy().into_owned())
            .or_default();

        // Have to juggle the config a bit due to serialization constraints
        let mut output_conf: HashMap<_, _> = std::mem::take(config)
            .into_iter()
            .map(|x| ((x.report_id, x.start_bit), x.target))
            .collect();

        for rep in reports {
            let rep_id = rep.id();

            for out in rep.outputs.iter_mut() {
                let key = (rep_id, out.bits().start);
                let mut entry = output_conf.remove(&key);

                egui::ComboBox::from_label(out.name.as_ref().map(|x| x.as_str()).unwrap_or("Unk"))
                    .selected_text(
                        entry
                            .map(|x| format!("{x}"))
                            .unwrap_or_else(|| "None".to_string()),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut entry, None, "None");
                        for target in LightingTarget::iter() {
                            ui.selectable_value(&mut entry, Some(target), format!("{target}"));
                        }
                    });
                if let Some(entry) = entry {
                    output_conf.insert(key, entry);
                }
                let mut highlighted = self.highlighted_output == key;
                if ui.toggle_value(&mut highlighted, "Highlight").clicked() {
                    if highlighted {
                        self.highlighted_output = key;
                    } else {
                        self.highlighted_output = (u32::MAX, u32::MAX)
                    }
                }

                if highlighted {
                    out.real_value = blink_value();
                } else {
                    out.real_value = 0.0;
                }

                ui.end_row();
            }
            dev.write_report(rep);
        }

        *config = output_conf
            .into_iter()
            .map(|((report_id, start_bit), target)| MappedTarget {
                report_id,
                start_bit,
                target,
            })
            .collect();

        if ui.button("Close").clicked() {
            self.binding_device = None
        }
    }

    /// Blink all outputs on highlighted device
    fn blink_highlighted(&mut self) {
        if let Some((dev, _)) = &self.highlighted_device {
            let mut reports = dev.reports().ok();
            let t = blink_value();

            for ele in reports.iter_mut().flat_map(|x| x.iter_mut()) {
                for out in ele.outputs.iter_mut() {
                    out.real_value = t;
                }

                dev.write_report(ele);
            }
        }
    }
}

fn blink_value() -> f32 {
    let t = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("System time before epoch")
        .as_millis()
        % 1000;
    let t = t / 500;
    let t = t as f32;
    t
}

use std::{io::BufReader, path::Path};

use itertools::Itertools;
use serde::{de::Visitor, Deserialize};

use crate::{
    settings_screen::skin_select::SkinMeta,
    skin_settings::{ColorVisitor, SettingsColor, SkinSettingEntry},
};

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum LegacyConfigEntry {
    Text {
        label: Option<String>,
        default: String,
        #[serde(default)]
        secret: bool,
    },
    Selection {
        label: Option<String>,
        default: String,
        values: Vec<String>,
    },
    Color {
        label: Option<String>,
        default: String,
    },
    Bool {
        label: Option<String>,
        default: bool,
    },
    Float {
        label: Option<String>,
        default: f64,
        min: f64,
        max: f64,
    },
    Int {
        label: Option<String>,
        default: i64,
        min: i64,
        max: i64,
    },
    Label {},
    Separator {},
}

fn convert_from_legacy_entry(name: String, value: LegacyConfigEntry) -> SkinSettingEntry {
    match value {
        LegacyConfigEntry::Text {
            label,
            default,
            secret,
        } => SkinSettingEntry::Text {
            default,
            label,
            name,
            secret,
        },
        LegacyConfigEntry::Selection {
            label,
            default,
            values,
        } => SkinSettingEntry::Selection {
            default,
            label,
            name,
            values,
        },
        LegacyConfigEntry::Color { label, default } => SkinSettingEntry::Color {
            default: default.parse().unwrap_or_default(),
            label,
            name,
        },
        LegacyConfigEntry::Bool { label, default } => SkinSettingEntry::Bool {
            default,
            label,
            name,
        },
        LegacyConfigEntry::Float {
            label,
            default,
            min,
            max,
        } => SkinSettingEntry::Float {
            default,
            label,
            name,
            min,
            max,
        },
        LegacyConfigEntry::Int {
            label,
            default,
            min,
            max,
        } => SkinSettingEntry::Integer {
            default,
            label,
            name,
            min,
            max,
        },
        LegacyConfigEntry::Label {} => SkinSettingEntry::Label { v: name },
        LegacyConfigEntry::Separator {} => SkinSettingEntry::Separator,
    }
}

pub fn convert_legacy_config(reader: impl std::io::Read) -> anyhow::Result<Vec<SkinSettingEntry>> {
    let temp_map: serde_json::Map<String, serde_json::Value> = serde_json::from_reader(reader)?;
    let temp_map: Vec<(String, LegacyConfigEntry)> = temp_map
        .into_iter()
        .map(|(k, mut v)| {
            if k.starts_with("separator") {
                if let Some(object_mut) = v.as_object_mut() {
                    object_mut.insert(
                        "type".to_string(),
                        serde_json::Value::String("separator".to_string()),
                    );
                }
            }
            (k, v)
        })
        .map(|(k, v)| serde_json::from_value::<LegacyConfigEntry>(v).map(|v| (k, v)))
        .try_collect()?;

    Ok(temp_map
        .into_iter()
        .map(|(name, value)| convert_from_legacy_entry(name, value))
        .collect())
}

#[cfg(test)]
mod tests {
    use crate::util::legacy_skin_config::convert_legacy_config;

    #[test]
    fn convert_skin_config() {
        let legacy_config = r#"
            {
                    "Gameplay:" : { "type" : "label" },

                    "earlate_position": {
                            "type": "selection",
                            "label": "Early/Late display position",
                            "default": "bottom",
                            "values": ["bottom", "middle", "top", "off"]
                    },
                    "nick": {
                            "type" : "text",
                            "label" : "Display name",
                            "default" : "Guest"
                    },

                    "separator_a" : {},
                    "Song select:" : { "type" : "label" },
                    "show_guide": {
                            "label" : "Show control guide on song select",
                            "type": "bool",
                            "default": true
                    },
                    "separator_b" : {},
                    "Test objects:" : { "type" : "label" },
                    "Testing with space" : {
                            "type": "float",
                            "label": "Test setting with spaces in the key",
                            "default": 50.0,
                            "max": 100.0,
                            "min": -100.0
                    },

                    "Ineger_test" : {
                            "type": "int",
                            "label": "Ineger Test with range -100<->100",
                            "default": 50,
                            "max": 100,
                            "min": -100
                    },

                    "col_test" : {
                            "type": "color",
                            "label": "Color Test",
                            "default": "007FFFFF"
                    },

                    "secret_value" : {
                            "type" : "text",
                            "label" : "Secret value test",
                            "default" : "usc123",
                            "secret" : true
                    }
            }
            "#;

        convert_legacy_config(std::io::Cursor::new(legacy_config.as_bytes()))
            .expect("Failed to covnert legacy config");
    }
}

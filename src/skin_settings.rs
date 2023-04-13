use puffin_egui::egui::Color32;
use serde::{de::Visitor, Deserialize, Serialize};

#[derive(Debug)]
pub struct SettingsColor(Color32);

impl Serialize for SettingsColor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (r, g, b, a) = self.0.to_tuple();
        serializer.collect_str(&format!("{:02x}{:02x}{:02x}{:02x}", r, g, b, a))
    }
}

struct ColorVisitor;

impl<'de> Visitor<'de> for ColorVisitor {
    type Value = Color32;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a string containing exactly {} bytes", 2 * 4)
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if v.len() != 8 {
            Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(v),
                &self,
            ))
        } else {
            let r = u8::from_str_radix(&v[0..2], 16)
                .map_err(|e| serde::de::Error::custom(e.to_string()))?;
            let g = u8::from_str_radix(&v[2..4], 16)
                .map_err(|e| serde::de::Error::custom(e.to_string()))?;
            let b = u8::from_str_radix(&v[4..6], 16)
                .map_err(|e| serde::de::Error::custom(e.to_string()))?;
            let a = u8::from_str_radix(&v[6..8], 16)
                .map_err(|e| serde::de::Error::custom(e.to_string()))?;

            Ok(Color32::from_rgba_premultiplied(r, g, b, a))
        }
    }
}

impl<'de> Deserialize<'de> for SettingsColor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(SettingsColor(deserializer.deserialize_str(ColorVisitor)?))
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SkinSettingEntry {
    Label {
        v: String,
    },
    Separator,
    Selection {
        default: String,
        label: String,
        name: String,
        values: Vec<String>,
    },
    Text {
        default: String,
        label: String,
        name: String,
        #[serde(default)]
        secret: bool,
    },
    Color {
        default: SettingsColor,
        label: String,
        name: String,
    },

    Bool {
        default: bool,
        label: String,
        name: String,
    },

    Float {
        default: f32,
        label: String,
        name: String,
        min: f32,
        max: f32,
    },

    Integer {
        default: i32,
        label: String,
        name: String,
        min: i32,
        max: i32,
    },
}

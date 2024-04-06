use egui::Color32;
use serde::{de::Visitor, Deserialize, Serialize};
use tealr::{
    mlu::mlua::{FromLua, IntoLuaMulti, MultiValue, Value},
    ToTypename,
};

#[derive(Debug, Clone, Copy)]
pub struct SettingsColor(pub Color32);

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

#[derive(Debug, Serialize, Deserialize, Clone)]
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
        default: f64,
        label: String,
        name: String,
        min: f64,
        max: f64,
    },

    Integer {
        default: i64,
        label: String,
        name: String,
        min: i64,
        max: i64,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone, ToTypename)]
#[serde(untagged)]
pub enum SkinSettingValue {
    None,
    Integer(i64),
    Float(f64),
    Bool(bool),
    Color(SettingsColor),
    Text(String),
}

impl<'lua> FromLua<'lua> for SkinSettingValue {
    fn from_lua(
        lua_value: tealr::mlu::mlua::Value<'lua>,
        _: &'lua tealr::mlu::mlua::Lua,
    ) -> tealr::mlu::mlua::Result<Self> {
        match lua_value {
            tealr::mlu::mlua::Value::Nil => Ok(Self::None),
            tealr::mlu::mlua::Value::Boolean(b) => Ok(Self::Bool(b)),
            tealr::mlu::mlua::Value::Integer(n) => Ok(Self::Integer(n)),
            tealr::mlu::mlua::Value::Number(n) => Ok(Self::Float(n)),
            tealr::mlu::mlua::Value::String(s) => Ok(Self::Text(String::from(s.to_str()?))),
            tealr::mlu::mlua::Value::Table(t) => {
                let a: Result<Vec<u8>, _> = t.sequence_values::<u8>().collect();
                let a = a?;

                if a.len() == 4 {
                    Ok(Self::Color(SettingsColor(
                        Color32::from_rgba_premultiplied(a[0], a[1], a[2], a[3]),
                    )))
                } else if a.len() == 3 {
                    Ok(Self::Color(SettingsColor(Color32::from_rgb(
                        a[0], a[1], a[2],
                    ))))
                } else {
                    Err(tealr::mlu::mlua::Error::FromLuaConversionError {
                        from: "table",
                        to: "SkinSettingValue::Color",
                        message: Some("Not a color array".to_string()),
                    })
                }
            }
            v => Err(tealr::mlu::mlua::Error::FromLuaConversionError {
                from: v.type_name(),
                to: "SkinSettingValue",
                message: None,
            }),
        }
    }
}

impl<'lua> IntoLuaMulti<'lua> for SkinSettingValue {
    fn into_lua_multi(
        self,
        lua: &'lua tealr::mlu::mlua::Lua,
    ) -> tealr::mlu::mlua::Result<tealr::mlu::mlua::MultiValue<'lua>> {
        match self {
            SkinSettingValue::Color(c) => Ok(MultiValue::from_vec(vec![
                Value::Integer(c.0.r() as i64),
                Value::Integer(c.0.g() as i64),
                Value::Integer(c.0.b() as i64),
                Value::Integer(c.0.a() as i64),
            ])),
            SkinSettingValue::None => Ok(MultiValue::default()),
            SkinSettingValue::Integer(v) => Ok(MultiValue::from_vec(vec![Value::Integer(v)])),
            SkinSettingValue::Float(v) => Ok(MultiValue::from_vec(vec![Value::Number(v)])),
            SkinSettingValue::Bool(v) => Ok(MultiValue::from_vec(vec![Value::Boolean(v)])),
            SkinSettingValue::Text(v) => Ok(MultiValue::from_vec(vec![Value::String(
                lua.create_string(&v)?,
            )])),
        }
    }
}

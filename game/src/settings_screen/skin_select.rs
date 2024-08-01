use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SkinMeta {
    pub name: String,
    pub description: String,
    pub skin_version: String,
    pub game_version: String,
    pub author: String,
    pub website: String,
}

impl SkinMeta {
    pub fn named(s: impl Into<String>) -> Self {
        Self {
            name: s.into(),
            ..Default::default()
        }
    }
}

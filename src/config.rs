use std::{path::PathBuf, sync::RwLock};

use log::{error, info};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct GameConfig {
    #[serde(skip_serializing, skip_deserializing)]
    config_file: PathBuf,
    pub songs_path: PathBuf,
    pub skin: String,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            config_file: PathBuf::from_iter([".", "Main.cfg"]),
            songs_path: PathBuf::from_iter([".", "songs"]),
            skin: "Default".into(),
        }
    }
}

static INSTANCE: OnceCell<RwLock<GameConfig>> = OnceCell::new();

impl GameConfig {
    pub fn get() -> Option<std::sync::RwLockReadGuard<'static, GameConfig>> {
        INSTANCE.get().and_then(|i| i.read().ok())
    }
    pub fn get_mut() -> Option<std::sync::RwLockWriteGuard<'static, GameConfig>> {
        INSTANCE.get().and_then(|i| i.write().ok())
    }
    pub fn init(path: PathBuf) {
        info!("Loading game config from: {:?}", &path);
        let file_content =
            std::fs::read_to_string(&path).map(|str| toml::from_str::<GameConfig>(&str));

        match file_content {
            Ok(Ok(mut config)) => {
                config.config_file = path;
                INSTANCE.set(RwLock::new(config));
            }
            Ok(Err(e)) => {
                error!("{}", e);
                INSTANCE.set(RwLock::new(GameConfig {
                    config_file: path,
                    songs_path: PathBuf::from_iter([".", "songs"]),
                    skin: "Default".into(),
                }));
            }
            Err(e) => {
                error!("{}", e);
                INSTANCE.set(RwLock::new(GameConfig {
                    config_file: path,
                    songs_path: PathBuf::from_iter([".", "songs"]),
                    skin: "Default".into(),
                }));
            }
        }
    }

    pub fn save(&self) {
        info!("Saving config");

        if let Ok(data) = toml::to_string_pretty(self) {
            std::fs::write(&self.config_file, data);
        }
    }
}

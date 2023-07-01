use std::{collections::HashMap, fs::File, io::Read, path::PathBuf, sync::RwLock};

use clap::Parser;
use log::{error, info};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};

use crate::skin_settings::{SkinSettingEntry, SkinSettingValue};

#[derive(Debug, Default, Parser)]
pub struct Args {
    pub chart: Option<String>,
    #[arg(short, long)]
    pub debug: bool,
    #[arg(short, long)]
    pub sound_test: bool,
    #[arg(short, long)]
    pub profiling: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GameConfig {
    #[serde(skip_serializing, skip_deserializing)]
    config_file: PathBuf,
    pub songs_path: PathBuf,
    pub skin: String,
    pub laser_hues: [f32; 2],
    #[serde(skip_serializing, skip_deserializing)]
    pub skin_settings: HashMap<String, SkinSettingValue>,
    #[serde(skip_serializing, skip_deserializing)]
    pub game_folder: PathBuf,
    #[serde(skip_serializing, skip_deserializing)]
    pub args: Args,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            config_file: PathBuf::from_iter([".", "Main.cfg"]),
            songs_path: PathBuf::from_iter([".", "songs"]),
            skin: "Default".into(),
            skin_settings: HashMap::new(),
            laser_hues: [200.0, 330.0],
            game_folder: std::env::current_dir().unwrap(),
            args: Default::default(),
        }
    }
}

static INSTANCE: OnceCell<RwLock<GameConfig>> = OnceCell::new();

impl GameConfig {
    pub fn get() -> std::sync::RwLockReadGuard<'static, GameConfig> {
        INSTANCE
            .get()
            .and_then(|i| i.read().ok())
            .expect("Tried to get GameConfig before initializing")
    }
    pub fn get_mut() -> std::sync::RwLockWriteGuard<'static, GameConfig> {
        INSTANCE
            .get()
            .and_then(|i| i.write().ok())
            .expect("Tried to get GameConfig before initializing")
    }

    pub fn skin_path(&self) -> PathBuf {
        let mut skin_path = self.config_file.clone();
        skin_path.pop();
        skin_path.push("skins");
        skin_path.push(&self.skin);
        skin_path
    }

    fn skin_config_path(&self) -> PathBuf {
        let mut skin_config_path = self.config_file.clone();
        skin_config_path.pop();
        skin_config_path.push("skins");
        skin_config_path.push(&self.skin);
        skin_config_path.push("skin_config.cfg");
        skin_config_path
    }

    fn init_skin_settings(&mut self) -> anyhow::Result<()> {
        let definition_path = self
            .skin_config_path()
            .with_file_name("config-definitions.json");

        let file = File::open(definition_path)?;
        let definitions: Vec<SkinSettingEntry> = serde_json::from_reader(file)?;

        for def in definitions {
            let entry = match def {
                SkinSettingEntry::Selection {
                    default,
                    label: _,
                    name,
                    values: _,
                } => (name, SkinSettingValue::Text(default)),
                SkinSettingEntry::Text {
                    default,
                    label: _,
                    name,
                    secret: _,
                } => (name, SkinSettingValue::Text(default)),
                SkinSettingEntry::Color {
                    default,
                    label: _,
                    name,
                } => (name, SkinSettingValue::Color(default)),
                SkinSettingEntry::Bool {
                    default,
                    label: _,
                    name,
                } => (name, SkinSettingValue::Bool(default)),
                SkinSettingEntry::Float {
                    default,
                    label: _,
                    name,
                    min: _,
                    max: _,
                } => (name, SkinSettingValue::Float(default)),
                SkinSettingEntry::Integer {
                    default,
                    label: _,
                    name,
                    min: _,
                    max: _,
                } => (name, SkinSettingValue::Integer(default)),
                _ => continue,
            };

            self.skin_settings.insert(entry.0, entry.1);
        }

        let mut file = File::open(self.skin_config_path())?;
        let mut skin_settings_string = String::new();
        file.read_to_string(&mut skin_settings_string)?;

        let skin_settings: HashMap<String, SkinSettingValue> =
            toml::from_str(&skin_settings_string)?;

        for (k, v) in skin_settings {
            self.skin_settings.insert(k, v);
        }

        Ok(())
    }

    pub fn init(path: PathBuf, args: Args) {
        info!("Loading game config from: {:?}", &path);
        let file_content =
            std::fs::read_to_string(&path).map(|str| toml::from_str::<GameConfig>(&str));

        match file_content {
            Ok(Ok(mut config)) => {
                config.args = args;
                config.config_file = path;
                INSTANCE.set(RwLock::new(config));
            }
            Ok(Err(e)) => {
                error!("{}", e);
                INSTANCE.set(RwLock::new(GameConfig {
                    config_file: path,
                    songs_path: PathBuf::from_iter([".", "songs"]),
                    skin: "Default".into(),
                    args,
                    ..Default::default()
                }));
            }
            Err(e) => {
                error!("{}", e);
                INSTANCE.set(RwLock::new(GameConfig {
                    config_file: path,
                    songs_path: PathBuf::from_iter([".", "songs"]),
                    skin: "Default".into(),
                    args,
                    ..Default::default()
                }));
            }
        }

        if let Err(err) = GameConfig::get_mut().init_skin_settings() {
            log::warn!("{:?}", err)
        };
    }

    pub fn save(&self) {
        info!("Saving config");

        if let Ok(data) = toml::to_string_pretty(self) {
            std::fs::write(&self.config_file, data);
        }

        if let Ok(data) = toml::to_string_pretty(&self.skin_settings) {
            std::fs::write(self.skin_config_path(), data);
        }
    }
}

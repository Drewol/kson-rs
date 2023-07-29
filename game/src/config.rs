use std::{collections::HashMap, fs::File, io::Read, path::PathBuf, sync::RwLock};

use clap::Parser;
use log::{error, info};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};

use crate::{
    button_codes::UscButton,
    skin_settings::{SkinSettingEntry, SkinSettingValue},
};

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
    pub mappings: Vec<String>,
    pub mouse_knobs: bool,
    pub mouse_ppr: f64,
    pub keyboard_buttons: bool,
    pub keyboard_knobs: bool,
    #[serde(skip_serializing, skip_deserializing)]
    pub skin_settings: HashMap<String, SkinSettingValue>,
    #[serde(skip_serializing, skip_deserializing)]
    pub game_folder: PathBuf,
    #[serde(skip_serializing, skip_deserializing)]
    pub args: Args,
    pub keybinds: Vec<Keybinds>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct Keybinds {
    bt_a: u32,
    bt_b: u32,
    bt_c: u32,
    bt_d: u32,
    fx_l: u32,
    fx_r: u32,
    start: u32,
    back: u32,
    laser_l: (u32, u32),
    laser_r: (u32, u32),
}

impl Keybinds {
    pub fn match_button(&self, key: u32) -> Option<UscButton> {
        let Keybinds {
            bt_a,
            bt_b,
            bt_c,
            bt_d,
            fx_l,
            fx_r,
            start,
            back,
            laser_l: (ll_l, ll_r),
            laser_r: (rl_l, rl_r),
        } = self;

        //TODO: Better way?
        match &key {
            k if k == bt_a => Some(UscButton::BT(kson::BtLane::A)),
            k if k == bt_b => Some(UscButton::BT(kson::BtLane::B)),
            k if k == bt_c => Some(UscButton::BT(kson::BtLane::C)),
            k if k == bt_d => Some(UscButton::BT(kson::BtLane::D)),
            k if k == fx_l => Some(UscButton::FX(kson::Side::Left)),
            k if k == fx_r => Some(UscButton::FX(kson::Side::Right)),
            k if k == start => Some(UscButton::Start),
            k if k == back => Some(UscButton::Back),
            k if k == ll_l => Some(UscButton::Laser(kson::Side::Left, kson::Side::Left)),
            k if k == ll_r => Some(UscButton::Laser(kson::Side::Left, kson::Side::Right)),
            k if k == rl_l => Some(UscButton::Laser(kson::Side::Right, kson::Side::Left)),
            k if k == rl_r => Some(UscButton::Laser(kson::Side::Right, kson::Side::Right)),
            _ => None,
        }
    }
}

impl Default for Keybinds {
    fn default() -> Self {
        Self {
            bt_a: 32,          // D
            bt_b: 33,          // F
            bt_c: 36,          // J
            bt_d: 37,          // K
            fx_l: 46,          // C
            fx_r: 50,          // M
            start: 2,          // 1
            back: 1,           // Esc
            laser_l: (17, 18), // (W,E)
            laser_r: (24, 25), // (O,P)
        }
    }
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            config_file: PathBuf::from_iter([".", "Main.cfg"]),
            songs_path: PathBuf::from_iter([".", "songs"]),
            skin: "Default".into(),
            skin_settings: HashMap::new(),
            laser_hues: [200.0, 330.0],
            game_folder: crate::default_game_dir(),
            args: Default::default(),
            mappings: vec![
            String::from("03000000d01600006d0a000000000000,Pocket Voltex Rev4,a:b1,b:b2,y:b3,x:b4,leftshoulder:b5,rightshoulder:b6,start:b0,leftx:a0,rightx:a1"),
            String::from("03000000cf1c00001410000000000000,F2 eAcloud,a:b1,b:b2,x:b4,y:b3,start:b0,leftshoulder:b5,rightshoulder:b6,leftx:a0,rightx:a1"),
            String::from("030000008f0e00001811000000000000,F2 HID,a:b1,b:b2,x:b4,y:b3,back:b7,start:b0,leftshoulder:b5,rightshoulder:b6,leftx:a0,rightx:a1")
            ],
            mouse_knobs: false,
            mouse_ppr: 256.0,
            keyboard_buttons: false,
            keybinds: vec![Keybinds::default()],
            keyboard_knobs: false,

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

    pub fn init(mut path: PathBuf, args: Args) {
        info!("Loading game config from: {:?}", &path);
        let file_content =
            std::fs::read_to_string(&path).map(|str| toml::from_str::<GameConfig>(&str));

        match file_content {
            Ok(Ok(mut config)) => {
                config.args = args;
                config.config_file = path.clone();
                path.pop();
                config.game_folder = path;
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

        match toml::to_string_pretty(self) {
            Ok(data) => {
                std::fs::write(&self.config_file, data);
            }
            Err(e) => error!("Could not save config: {e}"),
        }

        match toml::to_string_pretty(&self.skin_settings) {
            Ok(data) => {
                std::fs::write(self.skin_config_path(), data);
            }
            Err(e) => error!("Could not save skin config: {e}"),
        }
    }
}

pub mod migrations;

use std::default;
use std::{
    collections::HashMap, fmt::Display, fs::File, io::Read, path::PathBuf, sync::RwLock,
    time::Duration,
};

use clap::Parser;
use log::{error, info};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::keyboard::PhysicalKey;

use crate::{
    button_codes::{CustomBindings, UscButton},
    game::{self, HitWindow},
    skin_settings::{SkinSettingEntry, SkinSettingValue},
    song_provider,
};
use serde_with::serde_as;

#[derive(Debug, Default, Parser, Clone)]
pub struct Args {
    pub chart: Option<String>,
    #[arg(short, long)]
    pub debug: bool,
    #[arg(short, long)]
    pub sound_test: bool,
    #[arg(short, long)]
    pub profiling: bool,
    #[arg(long)]
    pub notitle: bool,
    #[arg(long)]
    pub camera_test: bool,
    #[arg(long)]
    pub settings: bool,
    #[arg(long)]
    pub companion_schema: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScoreDisplayMode {
    #[default]
    Additive,
    Subtractive,
    Average,
}

impl Display for ScoreDisplayMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ScoreDisplayMode::Additive => "Additive",
            ScoreDisplayMode::Subtractive => "Subtractive",
            ScoreDisplayMode::Average => "Average",
        })
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, Default, PartialEq)]
pub enum ScoreScreenshot {
    #[default]
    Never,
    Highscores,
    Always,
}

impl Display for ScoreScreenshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScoreScreenshot::Never => f.write_str("Never"),
            ScoreScreenshot::Highscores => f.write_str("Highscores"),
            ScoreScreenshot::Always => f.write_str("Always"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde_as]
#[serde(default)]
pub struct GameConfig {
    #[serde(skip_serializing, skip_deserializing)]
    config_file: PathBuf,
    pub songs_path: PathBuf,
    pub skin: String,
    pub laser_hues: [f32; 2],
    pub mappings: Vec<String>,
    pub mouse_knobs: bool,
    pub mouse_ppr: f64,
    pub mod_speed: f64,
    pub keyboard_buttons: bool,
    pub keyboard_knobs: bool,
    pub global_offset: i32,
    pub button_offset: i32,
    pub laser_offset: i32,
    #[serde(skip_serializing, skip_deserializing)]
    pub skin_definition: Vec<SkinSettingEntry>,
    #[serde(skip_serializing, skip_deserializing)]
    pub skin_settings: HashMap<String, SkinSettingValue>,
    #[serde(skip_serializing, skip_deserializing)]
    pub game_folder: PathBuf,
    #[serde(skip_serializing, skip_deserializing)]
    pub args: Args,
    pub keybinds: Vec<Keybinds>,
    pub controller_binds: CustomBindings,
    pub song_select: SongSelectSettings,
    pub graphics: GraphicsSettings,
    #[serde_as(as = "DurationMilliSecondsWithFrac<f64>")]
    pub laser_input_delay: Duration,
    pub distant_button_scale: f32,
    pub master_volume: f32,
    pub hit_window: game::HitWindow,
    pub score_display: ScoreDisplayMode,
    pub fallback_gauge: bool,
    pub start_gauge: game::gauge::GaugeType,
    pub slam_volume: f32,
    pub companion_address: Option<String>,
    pub score_screenshots: ScoreScreenshot,
    pub screenshot_path: PathBuf,
    pub ir_endpoint: String,
    pub ir_api_token: String,
    pub multiplayer: MultiplayerSettings,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MultiplayerSettings {
    pub server: String,
    pub name: String,
}

impl Default for MultiplayerSettings {
    fn default() -> Self {
        Self {
            server: "usc-multi.drewol.me:39079".into(),
            name: Default::default(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum Fullscreen {
    Windowed {
        pos: PhysicalPosition<i32>,
        size: PhysicalSize<u32>,
    },
    Borderless {
        monitor: PhysicalPosition<i32>,
    },
    Exclusive {
        monitor: PhysicalPosition<i32>,
        resolution: PhysicalSize<u32>,
    },
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(default)]
pub struct GraphicsSettings {
    pub fullscreen: Fullscreen,
    pub vsync: bool,
    pub anti_alias: u8,
    pub target_fps: u32,
    pub show_fps: bool,
    pub disable_bg: bool,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            fullscreen: Fullscreen::Windowed {
                pos: PhysicalPosition::new(0, 0),
                size: PhysicalSize::new(1280, 720),
            },
            vsync: true,
            anti_alias: 4,
            target_fps: 300,
            show_fps: false,
            disable_bg: false,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(default)]
pub struct SongSelectSettings {
    pub sorting: song_provider::SongSort,
    pub filter: song_provider::SongFilter,
    pub last_played: song_provider::SongDiffId,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(default)]
pub struct Keybinds {
    bt_a: PhysicalKey,
    bt_b: PhysicalKey,
    bt_c: PhysicalKey,
    bt_d: PhysicalKey,
    fx_l: PhysicalKey,
    fx_r: PhysicalKey,
    start: PhysicalKey,
    back: PhysicalKey,
    refresh: PhysicalKey,
    laser_l: (PhysicalKey, PhysicalKey),
    laser_r: (PhysicalKey, PhysicalKey),
    reload_scripts: PhysicalKey,
}

impl Keybinds {
    pub fn get(&self, button: UscButton) -> Option<PhysicalKey> {
        match button {
            UscButton::BT(bt_lane) => Some(match bt_lane {
                kson::BtLane::A => self.bt_a,
                kson::BtLane::B => self.bt_b,
                kson::BtLane::C => self.bt_c,
                kson::BtLane::D => self.bt_d,
            }),
            UscButton::FX(side) => Some(match side {
                kson::Side::Left => self.fx_l,
                kson::Side::Right => self.fx_r,
            }),
            UscButton::Start => Some(self.start),
            UscButton::Back => Some(self.back),
            UscButton::Refresh => Some(self.refresh),
            UscButton::Laser(side, dir) => Some(match (side, dir) {
                (kson::Side::Left, kson::Side::Left) => self.laser_l.0,
                (kson::Side::Left, kson::Side::Right) => self.laser_l.1,
                (kson::Side::Right, kson::Side::Left) => self.laser_r.0,
                (kson::Side::Right, kson::Side::Right) => self.laser_r.1,
            }),
            UscButton::Other(_) => None,
        }
    }

    pub fn set(&mut self, button: UscButton, key: PhysicalKey) {
        let b = match button {
            UscButton::BT(bt_lane) => Some(match bt_lane {
                kson::BtLane::A => &mut self.bt_a,
                kson::BtLane::B => &mut self.bt_b,
                kson::BtLane::C => &mut self.bt_c,
                kson::BtLane::D => &mut self.bt_d,
            }),
            UscButton::FX(side) => Some(match side {
                kson::Side::Left => &mut self.fx_l,
                kson::Side::Right => &mut self.fx_r,
            }),
            UscButton::Start => Some(&mut self.start),
            UscButton::Back => Some(&mut self.back),
            UscButton::Refresh => Some(&mut self.refresh),
            UscButton::Laser(side, dir) => Some(match (side, dir) {
                (kson::Side::Left, kson::Side::Left) => &mut self.laser_l.0,
                (kson::Side::Left, kson::Side::Right) => &mut self.laser_l.1,
                (kson::Side::Right, kson::Side::Left) => &mut self.laser_r.0,
                (kson::Side::Right, kson::Side::Right) => &mut self.laser_r.1,
            }),
            UscButton::Other(_) => None,
        };
        if let Some(b) = b {
            *b = key;
        }
    }

    pub fn is_reload_script(&self, key: PhysicalKey) -> bool {
        key == self.reload_scripts
    }

    pub fn match_button(&self, key: PhysicalKey) -> Option<UscButton> {
        let Keybinds {
            bt_a,
            bt_b,
            bt_c,
            bt_d,
            fx_l,
            fx_r,
            start,
            back,
            refresh,
            laser_l: (ll_l, ll_r),
            laser_r: (rl_l, rl_r),
            reload_scripts: _,
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
            k if k == refresh => Some(UscButton::Refresh),
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
        use winit::keyboard::KeyCode;
        Self {
            bt_a: PhysicalKey::Code(KeyCode::KeyD),    // D
            bt_b: PhysicalKey::Code(KeyCode::KeyF),    // F
            bt_c: PhysicalKey::Code(KeyCode::KeyJ),    // J
            bt_d: PhysicalKey::Code(KeyCode::KeyK),    // K
            fx_l: PhysicalKey::Code(KeyCode::KeyC),    // C
            fx_r: PhysicalKey::Code(KeyCode::KeyM),    // M
            start: PhysicalKey::Code(KeyCode::Digit1), // 1
            back: PhysicalKey::Code(KeyCode::Escape),  // Esc
            refresh: PhysicalKey::Code(KeyCode::F5),   // F5
            laser_l: (
                PhysicalKey::Code(KeyCode::KeyW),
                PhysicalKey::Code(KeyCode::KeyE),
            ), // (W,E)
            laser_r: (
                PhysicalKey::Code(KeyCode::KeyO),
                PhysicalKey::Code(KeyCode::KeyP),
            ), // (O,P)
            reload_scripts: PhysicalKey::Code(KeyCode::F9),
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
            skin_definition: vec![],
            mod_speed: 400.0,
            laser_hues: [200.0, 330.0],
            game_folder: crate::installer::default_game_dir(),
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
            global_offset: 0,
            button_offset: 0,
            laser_offset: 0,
            controller_binds: HashMap::new(),
            song_select: SongSelectSettings::default(),
            graphics: GraphicsSettings::default(),
            distant_button_scale: 2.0,
            master_volume: 0.8,
            hit_window: HitWindow::NORMAL,
            score_display: ScoreDisplayMode::default(),
            fallback_gauge: false,
            start_gauge: game::gauge::GaugeType::Normal,
            slam_volume: 0.75,
            laser_input_delay: Duration::from_millis(50),
            companion_address: Some("127.0.0.1:9002".to_string()),
            score_screenshots: ScoreScreenshot::default(),
            screenshot_path: PathBuf::from_iter([".", "screenshots"]),
            ir_api_token: String::new(),
            ir_endpoint: String::new(),
            multiplayer: MultiplayerSettings::default()
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
        let mut skin_path = self.game_folder.clone();
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

        for def in &definitions {
            let entry = match def {
                SkinSettingEntry::Selection {
                    default,
                    label: _,
                    name,
                    values: _,
                } => (name, SkinSettingValue::Text(default.clone())),
                SkinSettingEntry::Text {
                    default,
                    label: _,
                    name,
                    secret: _,
                } => (name, SkinSettingValue::Text(default.clone())),
                SkinSettingEntry::Color {
                    default,
                    label: _,
                    name,
                } => (name, SkinSettingValue::Color(*default)),
                SkinSettingEntry::Bool {
                    default,
                    label: _,
                    name,
                } => (name, SkinSettingValue::Bool(*default)),
                SkinSettingEntry::Float {
                    default,
                    label: _,
                    name,
                    min: _,
                    max: _,
                } => (name, SkinSettingValue::Float(*default)),
                SkinSettingEntry::Integer {
                    default,
                    label: _,
                    name,
                    min: _,
                    max: _,
                } => (name, SkinSettingValue::Integer(*default)),
                _ => continue,
            };

            self.skin_settings.insert(entry.0.clone(), entry.1);
        }

        let mut file = File::open(self.skin_config_path())?;
        let mut skin_settings_string = String::new();
        file.read_to_string(&mut skin_settings_string)?;

        let skin_settings: HashMap<String, SkinSettingValue> =
            toml::from_str(&skin_settings_string)?;

        for (k, v) in skin_settings {
            self.skin_settings.insert(k, v);
        }

        self.skin_definition = definitions;

        Ok(())
    }

    pub fn init(mut path: PathBuf, args: Args) {
        info!("Loading game config from: {:?}", &path);
        let file_content =
            std::fs::read_to_string(&path).map(|str| toml::from_str::<GameConfig>(&str));

        let instance_result = match file_content {
            Ok(Ok(mut config)) => {
                config.args = args;
                config.config_file.clone_from(&path);
                path.pop();
                config.game_folder = path;
                INSTANCE.set(RwLock::new(config))
            }
            Ok(Err(e)) => {
                error!("{}", e);
                INSTANCE.set(RwLock::new(GameConfig {
                    config_file: path,
                    songs_path: PathBuf::from_iter([".", "songs"]),
                    skin: "Default".into(),
                    args,
                    ..Default::default()
                }))
            }
            Err(e) => {
                error!("{}", e);
                INSTANCE.set(RwLock::new(GameConfig {
                    config_file: path,
                    songs_path: PathBuf::from_iter([".", "songs"]),
                    skin: "Default".into(),
                    args,
                    ..Default::default()
                }))
            }
        };

        instance_result.expect("Config already initialized");

        if let Err(err) = GameConfig::get_mut().init_skin_settings() {
            log::warn!("{}", err)
        };

        migrations::migrate_config();
    }

    pub fn save(&self) {
        info!("Saving config: {:?}", &self.config_file);

        if let Err(e) = toml::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!(e))
            .and_then(|data| {
                std::fs::write(&self.config_file, data).map_err(|e| anyhow::anyhow!(e))
            })
        {
            error!("Could not save config: {e}")
        }

        if let Err(e) = toml::to_string_pretty(&self.skin_settings)
            .map_err(|e| anyhow::anyhow!(e))
            .and_then(|data| {
                std::fs::write(self.skin_config_path(), data).map_err(|e| anyhow::anyhow!(e))
            })
        {
            error!("Could not save skin config: {e}")
        }
    }
}

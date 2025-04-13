use crate::{
    button_codes::{UscButton, UscInputEvent},
    config::{GameConfig, ScoreDisplayMode},
    game_main::AutoPlay,
    input_state::InputState,
    log_result,
    lua_service::LuaProvider,
    scene::{Scene, SceneData},
    shaded_mesh::ShadedMesh,
    songselect::Song,
    vg_ui::Vgfx,
    ControlMessage,
};

use anyhow::{anyhow, ensure, Result};
use di::{RefMut, ServiceProvider};
use egui::epaint::Hsva;
use egui_plot::{Line, PlotPoints};
use image::GenericImageView;
use itertools::Itertools;
use kson::{
    effects::AudioEffect,
    score_ticks::{PlacedScoreTick, ScoreTick, ScoreTickSummary, ScoreTicker},
    Chart, Graph, Side,
};
use kson_music_playback::GetBiQuadState;
use kson_rodio_sources::{
    biquad::{biquad, BiQuadState, BiQuadType, BiquadController},
    owned_source::{self, owned_source},
};

use log::{info, warn};
use mlua::{Function, Lua, LuaSerdeExt};
use puffin::{profile_function, profile_scope};
use rodio::{dynamic_mixer::DynamicMixerController, source::Buffered, Decoder, Source};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, VecDeque},
    f32::consts::SQRT_2,
    ops::Sub,
    path::PathBuf,
    rc::Rc,
    sync::{mpsc::Sender, Arc},
    time::{Duration, SystemTime},
};
use three_d::{vec2, vec3, Blend, Camera, Mat4, Matrix4, Vec3, Vec4, Viewport, Zero};
use three_d_asset::vec4;

pub mod chart_view;
use chart_view::*;
pub mod camera;
use camera::*;
mod background;
use background::GameBackground;
mod lua_data;
pub use lua_data::HitWindow;
pub(crate) use lua_data::LuaGameState;
pub mod graphics;

const LASER_THRESHOLD: f64 = 1.0 / 12.0;
const LEADIN: Duration = Duration::from_secs(3);

pub struct Game {
    view: ChartView,
    chart: kson::Chart,
    zero_time: SystemTime,
    duration_secs: f32,
    duration_ticks: u32,
    fx_long_shaders: ShadedMesh,
    bt_long_shaders: ShadedMesh,
    fx_chip_shaders: ShadedMesh,
    laser_shaders: [[ShadedMesh; 2]; 2], //[[left, left_current], [right, right_current]]
    track_shader: ShadedMesh,
    bt_chip_shader: ShadedMesh,
    lane_beam_shader: ShadedMesh,
    camera: ChartCamera,
    lua_game_state: lua_data::LuaGameState,
    hit_window: HitWindow,
    lua: Rc<Lua>,
    intro_done: bool,
    song: Arc<Song>,
    diff_idx: usize,
    control_tx: Option<Sender<ControlMessage>>,
    gauge: Gauges,
    results_requested: bool,
    closed: bool,
    playback: kson_music_playback::AudioPlayback,
    score_ticks: Vec<PlacedScoreTick>,
    score_current_max: u64,
    score_summary: ScoreTickSummary,
    score_display: ScoreDisplayMode,
    real_score: u64,
    display_score: u64,
    combo: u64,
    max_combo: u64,
    tick_queue: VecDeque<u32>,
    input_state: InputState,
    laser_cursors: [f64; 2],
    laser_active: [bool; 2],
    laser_wide: [u32; 2],
    laser_target: [Option<f64>; 2],
    laser_assist_ticks: [u8; 2],
    laser_alert: [usize; 2],
    laser_latest_dir_inputs: [[SystemTime; 2]; 2], //last left/right turn timestamps for both knobs, for checking slam hits
    laser_colors: [Vec4; 2],
    beam_colors: Vec<Vec4>,
    beam_colors_current: [[f32; 4]; 6],
    draw_axis_guides: bool,
    target_roll: TargetRoll,
    current_roll: f64,
    hit_ratings: Vec<HitRating>,
    mixer: Arc<DynamicMixerController<f32>>,
    biquad_control: BiquadController,
    source_owner: owned_source::Marker,
    slam_sample: Option<Buffered<Decoder<std::fs::File>>>,
    slam_marker: owned_source::Marker,
    background: Option<GameBackground>,
    foreground: Option<GameBackground>,
    service_provider: ServiceProvider,
    sync_delta: VecDeque<f64>,
    laser_effects: BTreeMap<u32, AudioEffect>,
    default_laser_effect: AudioEffect,
    autoplay: AutoPlay,
    slam_volume: f32,
    chip_h: f32,
    laser_buffer: [VecDeque<(SystemTime, f64)>; 2],
    laser_input_delay: Duration,
    laser_offset: f64,
    button_offset: f64,
    global_offset: f64,
}

#[derive(Clone, Copy)]
enum TargetRoll {
    None,
    Laser(f64),
    Manual(f64),
}
pub mod gauge;
use gauge::*;
#[derive(Debug, Clone, Copy)]
pub enum HitRating {
    None,
    Crit {
        tick: PlacedScoreTick,
        delta: f64,
        time: f64,
    },
    Good {
        tick: PlacedScoreTick,
        delta: f64,
        time: f64,
    },
    Miss {
        tick: PlacedScoreTick,
        delta: f64,
        time: f64,
    },
}

#[derive(Debug, Default, Clone, Copy)]
pub struct HitSummary {
    crit: u32,
    good: u32,
    miss: u32,
}

impl HitSummary {
    pub fn new(crit: u32, good: u32, miss: u32) -> Self {
        Self { crit, good, miss }
    }

    pub fn perfect(&self) -> bool {
        self.good == 0 && self.miss == 0
    }

    pub fn full_combo(&self) -> bool {
        self.miss == 0
    }
}

impl From<&[HitRating]> for HitSummary {
    fn from(value: &[HitRating]) -> Self {
        value
            .iter()
            .fold(Self::default(), |Self { crit, good, miss }, r| match r {
                HitRating::None => Self { crit, good, miss },
                HitRating::Crit { .. } => Self {
                    crit: crit + 1,
                    good,
                    miss,
                },
                HitRating::Good { .. } => Self {
                    crit,
                    good: good + 1,
                    miss,
                },
                HitRating::Miss { .. } => Self {
                    crit,
                    good,
                    miss: miss + 1,
                },
            })
    }
}

impl HitRating {
    pub fn delta(self) -> f64 {
        match self {
            HitRating::None => f64::NAN,
            HitRating::Crit { delta, .. }
            | HitRating::Good { delta, .. }
            | HitRating::Miss { delta, .. } => delta,
        }
    }

    pub fn time(self) -> f64 {
        match self {
            HitRating::None => f64::NAN,
            HitRating::Crit { time, .. }
            | HitRating::Good { time, .. }
            | HitRating::Miss { time, .. } => time,
        }
    }

    pub fn for_stats(self) -> bool {
        match self {
            HitRating::None => false,
            HitRating::Miss { tick, delta, .. } => {
                matches!(tick.tick, ScoreTick::Chip { .. }) && delta > 1.0
            }
            HitRating::Crit { tick, .. } | HitRating::Good { tick, .. } => {
                matches!(tick.tick, ScoreTick::Chip { .. })
            }
        }
    }

    pub fn crit(self) -> bool {
        match self {
            HitRating::None => true,
            HitRating::Crit { .. } => true,
            _ => false,
        }
    }

    pub fn hit(self) -> bool {
        !matches!(self, HitRating::Miss { .. })
    }
}

impl From<&Gauge> for lua_data::LuaGauge {
    fn from(value: &Gauge) -> Self {
        match value {
            Gauge::Normal { value, .. } => lua_data::LuaGauge {
                gauge_type: 0,
                options: 0,
                value: *value,
                name: "Normal".into(),
            },
            Gauge::Hard { value, .. } => lua_data::LuaGauge {
                gauge_type: 1,
                options: 0,
                value: *value,
                name: "Hard".into(),
            },
            Gauge::None => lua_data::LuaGauge {
                gauge_type: 0,
                options: 0,
                value: 0.0,
                name: "".to_string(),
            },
        }
    }
}

pub enum HoldState {
    Idle,
    Hit,
    Miss,
}

pub struct GameData {
    song: Arc<Song>,
    diff_idx: usize,
    chart: kson::Chart,
    skin_folder: PathBuf,
    audio: std::boxed::Box<(dyn rodio::source::Source<Item = f32> + std::marker::Send + 'static)>,
    autoplay: AutoPlay,
    song_folder: Option<PathBuf>,
}

impl GameData {
    pub fn new(
        song: Arc<Song>,
        diff_idx: usize,
        chart: kson::Chart,
        skin_folder: PathBuf,
        audio: Box<dyn Source<Item = f32> + Send>,
        autoplay: AutoPlay,
        song_folder: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        //TODO: Does not belong in game crate
        //TODO: Sort effects for proper overlapping sounds
        //TODO: Effects are added quickly now but render slowly as most the effects run for the whole song even when mixed to 0

        Ok(Self {
            chart,
            skin_folder,
            diff_idx,
            song,
            audio: Box::new(audio),
            autoplay,
            song_folder,
        })
    }
}

impl SceneData for GameData {
    fn make_scene(
        self: Box<Self>,
        service_provider: ServiceProvider,
    ) -> anyhow::Result<Box<dyn Scene>> {
        let Self {
            chart,
            skin_folder,
            diff_idx,
            song,
            audio,
            autoplay,
            song_folder,
        } = *self;
        profile_function!();

        let context = service_provider
            .get_required::<three_d::Context>()
            .as_ref()
            .clone();

        let mesh_transform = Mat4::from_scale(1.0);

        let mut shader_folder = skin_folder.clone();
        let mut texture_folder = skin_folder.clone();
        shader_folder.push("shaders");
        texture_folder.push("textures");
        texture_folder.push("dummy.png");

        let mut fx_long_shader = ShadedMesh::new(&context, "holdbutton", &shader_folder)
            .expect("Failed to load shader:")
            .with_transform(Matrix4::from_translation(vec3(-0.5, 0.0, 0.0)));

        let mut beam_shader = ShadedMesh::new(&context, "sprite", &shader_folder)
            .expect("Failed to load shader:")
            .with_transform(mesh_transform);

        beam_shader.use_texture(
            "mainTex",
            texture_folder.with_file_name("scorehit.png"),
            (false, false),
            false,
        )?;

        beam_shader.set_data_mesh(&graphics::xy_rect(Vec3::zero(), vec2(1.0, 1.0)));

        fx_long_shader.use_texture(
            "mainTex",
            texture_folder.with_file_name("fxbuttonhold.png"),
            (false, false),
            true,
        )?;

        fx_long_shader.set_data_mesh(&graphics::xy_rect(
            vec3(0.0, 0.5, 0.0),
            vec2(2.0 / 6.0, 1.0),
        ));

        let mut bt_long_shader = ShadedMesh::new(&context, "holdbutton", &shader_folder)
            .expect("Failed to load shader:")
            .with_transform(Matrix4::from_translation(vec3(-0.5, 0.0, 0.0)));

        bt_long_shader.use_texture(
            "mainTex",
            texture_folder.with_file_name("buttonhold.png"),
            (false, false),
            true,
        )?;

        bt_long_shader.set_data_mesh(&graphics::xy_rect(
            vec3(0.0, 0.5, 0.0),
            vec2(1.0 / 6.0, 1.0),
        ));

        let mut fx_chip_shader = ShadedMesh::new(&context, "button", &shader_folder)
            .expect("Failed to load shader:")
            .with_transform(Matrix4::from_translation(vec3(-0.5, 0.0, 0.0)));
        fx_chip_shader.use_texture(
            "mainTex",
            texture_folder.with_file_name("fxbutton.png"),
            (false, false),
            true,
        )?;
        let fx_height = 1.0;

        fx_chip_shader.set_data_mesh(&graphics::xy_rect(
            vec3(0.0, fx_height / 2.0, 0.0),
            vec2(2.0 / 6.0, fx_height),
        ));

        let mut bt_chip_shader = ShadedMesh::new(&context, "button", &shader_folder)
            .expect("Failed to load shader:")
            .with_transform(Matrix4::from_translation(vec3(-0.5, 0.0, 0.0)));
        let bt_height = 1.0;

        let bt_tex = bt_chip_shader.use_texture(
            "mainTex",
            texture_folder.with_file_name("button.png"),
            (false, false),
            true,
        )?;

        bt_chip_shader.set_data_mesh(&graphics::xy_rect(
            vec3(0.0, bt_height / 2.0, 0.0),
            vec2(1.0 / 6.0, bt_height),
        ));

        let chip_h = (1.0 / 6.0) * (bt_tex.height as f32 / bt_tex.width as f32);

        let mut track_shader =
            ShadedMesh::new(&context, "track", &shader_folder).expect("Failed to load shader:");
        track_shader.set_data_mesh(&graphics::xy_rect(
            Vec3::zero(),
            vec2(1.0, ChartView::TRACK_LENGTH * 2.0),
        ));

        let laser_colors: [three_d::Vector4<f32>; 2] = [
            Hsva::new(GameConfig::get().laser_hues[0] / 360.0, 1.0, 1.0, 1.0)
                .to_rgba_unmultiplied()
                .into(),
            Hsva::new(GameConfig::get().laser_hues[1] / 360.0, 1.0, 1.0, 1.0)
                .to_rgba_unmultiplied()
                .into(),
        ];

        track_shader.set_param("lCol", laser_colors[0]);
        track_shader.set_param("rCol", laser_colors[1]);

        track_shader.use_texture(
            "mainTex",
            texture_folder.with_file_name("track.png"),
            (false, false),
            true,
        )?;

        let mut laser_left =
            ShadedMesh::new(&context, "laser", &shader_folder).expect("Failed to load shader:");
        let mut laser_left_active =
            ShadedMesh::new(&context, "laser", &shader_folder).expect("Failed to load shader:");

        let mut laser_right =
            ShadedMesh::new(&context, "laser", &shader_folder).expect("Failed to load shader:");
        let mut laser_right_active =
            ShadedMesh::new(&context, "laser", &shader_folder).expect("Failed to load shader:");

        laser_left.use_texture(
            "mainTex",
            texture_folder.with_file_name("laser_l.png"),
            (false, true),
            true,
        )?;
        laser_left_active.use_texture(
            "mainTex",
            texture_folder.with_file_name("laser_l.png"),
            (false, true),
            true,
        )?;
        laser_right.use_texture(
            "mainTex",
            texture_folder.with_file_name("laser_r.png"),
            (false, true),
            true,
        )?;
        laser_right_active.use_texture(
            "mainTex",
            texture_folder.with_file_name("laser_r.png"),
            (false, true),
            true,
        )?;

        let beam_colors: Vec<_> = image::open(texture_folder.with_file_name("hitcolors.png"))
            .expect("Failed to load hitcolors.png")
            .pixels()
            .map(|x| x.2)
            .collect();

        laser_left.set_blend(Blend::ADD);
        laser_left_active.set_blend(Blend::ADD);
        laser_right.set_blend(Blend::ADD);
        laser_right_active.set_blend(Blend::ADD);

        let mut playback = kson_music_playback::AudioPlayback::new();
        let (biquad_control, _) = std::sync::mpsc::channel();
        playback
            .open(audio, "Game", None)
            .expect("Failed to load audio");
        playback.build_effects(&chart);
        playback.stop();
        let laser_effects = chart.laser_effect_queue();

        //TODO: No need to set leadin if first tick is beyond the leadin time.
        playback.set_leadin(LEADIN);

        let bg = chart
            .bg
            .legacy
            .as_ref()
            .and_then(|x| x.layer.as_ref())
            .and_then(|x| x.filename.clone())
            .unwrap_or_else(|| "fallback".to_string());

        let mut bg_folder = skin_folder.clone();
        bg_folder.push("backgrounds");
        bg_folder.push(bg);

        let mut bg_folders = vec![bg_folder.with_file_name("fallback"), bg_folder.clone()];

        if let Some(mut song_folder) = song_folder {
            if let Some(name) = chart
                .bg
                .legacy
                .as_ref()
                .and_then(|x| x.layer.as_ref())
                .and_then(|x| x.filename.as_ref())
            {
                song_folder.push(name);
                bg_folders.push(song_folder);
            }
        }

        bg_folders.reverse();

        let bg_enabled = !GameConfig::get().graphics.disable_bg;

        let background = bg_enabled
            .then(|| {
                bg_folders
                    .iter()
                    .filter_map(|x| {
                        GameBackground::new(
                            &context,
                            true,
                            x,
                            &chart,
                            service_provider.get_required(),
                            service_provider.get_required(),
                        )
                        .inspect_err(|e| warn!("Failed to load background from {x:?}: {e}"))
                        .inspect(|_| info!("Succefully loaded background from {x:?}"))
                        .ok()
                    })
                    .next()
            })
            .flatten();

        let foreground = bg_enabled
            .then(|| {
                bg_folders
                    .iter()
                    .filter_map(|x| {
                        GameBackground::new(
                            &context,
                            false,
                            x,
                            &chart,
                            service_provider.get_required(),
                            service_provider.get_required(),
                        )
                        .inspect_err(|e| warn!("Failed to load background from {x:?}: {e}"))
                        .inspect(|_| info!("Succefully loaded background from {x:?}"))
                        .ok()
                    })
                    .next()
            })
            .flatten();

        Ok(Box::new(Game::new(
            chart,
            &skin_folder,
            &context,
            fx_long_shader,
            bt_long_shader,
            fx_chip_shader,
            [
                [laser_left, laser_left_active],
                [laser_right, laser_right_active],
            ],
            track_shader,
            bt_chip_shader,
            beam_shader,
            song,
            diff_idx,
            playback,
            InputState::clone(&service_provider.get_required()),
            beam_colors,
            biquad_control,
            background,
            foreground,
            service_provider,
            laser_effects,
            autoplay,
            chip_h,
            laser_colors,
        )?))
    }
}

impl Game {
    pub fn new(
        chart: Chart,
        skin_root: &PathBuf,
        td: &three_d::Context,
        fx_long_shaders: ShadedMesh,

        bt_long_shaders: ShadedMesh,

        fx_chip_shaders: ShadedMesh,
        laser_shaders: [[ShadedMesh; 2]; 2],
        track_shader: ShadedMesh,
        bt_chip_shader: ShadedMesh,
        lane_beam_shader: ShadedMesh,
        song: Arc<Song>,
        diff_idx: usize,
        playback: kson_music_playback::AudioPlayback,
        input_state: InputState,
        beam_colors: Vec<image::Rgba<u8>>,
        biquad_control: BiquadController,
        background: Option<GameBackground>,
        foreground: Option<GameBackground>,
        service_provider: ServiceProvider,
        laser_effects: BTreeMap<u32, AudioEffect>,
        autoplay: AutoPlay,
        chip_h: f32,
        laser_colors: [three_d::Vector4<f32>; 2],
    ) -> Result<Self> {
        let mut view = ChartView::new(skin_root, td)?;
        view.build_laser_meshes(&chart);
        view.hispeed = (GameConfig::get().mod_speed
            / chart
                .mode_bpm()
                .ok_or(anyhow!("Failed to calculate Mode BPM"))?) as f32;
        let duration_secs = ((3000.0 + chart.tick_to_ms(chart.get_last_tick())) / 1000.0) as f32;

        let mut slam_path = skin_root.clone();
        slam_path.push("audio");
        slam_path.push("laser_slam.wav");

        let score_ticks = kson::score_ticks::generate_score_ticks(&chart);

        // Set up ticks to score for deterministic laser/hold scoring
        let last_tick = chart.get_last_tick();
        let last_processing_tick = chart.ms_to_tick(chart.tick_to_ms(last_tick) + 3000.0);
        let mut tick_queue = VecDeque::new();
        for i in 0.. {
            let scored_tick = chart.ms_to_tick(i as f64 * 1000.0 / 240.0);
            tick_queue.push_back(scored_tick);
            if scored_tick > last_processing_tick {
                break;
            }
        }

        let mut res = Self {
            song,
            diff_idx,
            intro_done: false,
            lua: LuaProvider::new_lua(),
            duration_ticks: last_tick,
            chart,
            view,
            duration_secs,
            zero_time: SystemTime::now(),
            bt_chip_shader,
            track_shader,
            bt_long_shaders,
            fx_chip_shaders,
            fx_long_shaders,
            laser_shaders,
            lane_beam_shader,
            camera: ChartCamera::new(),
            lua_game_state: lua_data::LuaGameState::default(),
            control_tx: None,
            results_requested: false,
            closed: false,
            playback,
            score_summary: score_ticks.summary(),
            score_current_max: 0,
            score_display: GameConfig::get().score_display,
            score_ticks,
            tick_queue,
            gauge: Gauges::default(),
            real_score: 0,
            display_score: u64::MAX,
            combo: 0,
            max_combo: 0,
            input_state,
            laser_cursors: [0.0, 1.0],
            laser_active: [false, false],
            laser_target: [None, None],
            laser_assist_ticks: [0; 2],
            laser_latest_dir_inputs: [[SystemTime::UNIX_EPOCH; 2]; 2],
            beam_colors: beam_colors
                .iter()
                .map(|x| {
                    let [r, g, b, a] = x.0;
                    vec4(r as f32, g as f32, b as f32, a as f32)
                })
                .collect(),
            beam_colors_current: [[0.0; 4]; 6],
            laser_colors,
            draw_axis_guides: false,
            current_roll: 0.0,
            target_roll: TargetRoll::None,
            hit_ratings: Vec::new(),
            mixer: service_provider.get_required(),
            biquad_control,
            background,
            foreground,
            source_owner: Default::default(),
            slam_sample: std::fs::File::open(slam_path)
                .ok()
                .and_then(|x| Decoder::new(x).ok())
                .map(|x| x.buffered()),
            slam_marker: Default::default(),
            service_provider,
            sync_delta: Default::default(),
            laser_wide: [0, 0],
            laser_alert: [0, 0],
            hit_window: GameConfig::get().hit_window,
            laser_effects,
            default_laser_effect: AudioEffect::PeakingFilter(
                kson::effects::PeakingFilter::default(),
            ),
            autoplay,
            slam_volume: GameConfig::get().slam_volume,
            chip_h,
            laser_buffer: [VecDeque::new(), VecDeque::new()],
            laser_input_delay: GameConfig::get().laser_input_delay,
            button_offset: -GameConfig::get().button_offset as _,
            global_offset: -GameConfig::get().global_offset as _,
            laser_offset: -GameConfig::get().laser_offset as _,
        };
        res.set_track_uniforms();
        Ok(res)
    }

    fn set_track_uniforms(&mut self) {
        [
            &mut self.track_shader,
            &mut self.fx_long_shaders,
            &mut self.bt_long_shaders,
            &mut self.fx_chip_shaders,
            &mut self.bt_chip_shader,
        ]
        .into_iter()
        .chain(self.laser_shaders.iter_mut().flatten())
        .for_each(|shader| {
            shader.set_param("trackPos", 0.0);
            shader.set_param("trackScale", 1.0);
            shader.set_param("hiddenCutoff", 0.0);
            shader.set_param("hiddenFadeWindow", 100.0);
            shader.set_param("suddenCutoff", 10.0);
            shader.set_param("suddenFadeWindow", 1000.0);
            shader.set_param("hitState", 1);
            shader.set_param("objectGlow", 0.6);
        });

        self.laser_shaders.iter_mut().flatten().for_each(|laser| {
            laser.set_param("objectGlow", 0.6);
            laser.set_param("hitState", 1);
        });
        self.laser_shaders[0]
            .iter_mut()
            .for_each(|ll| ll.set_param("color", self.laser_colors[0]));
        self.laser_shaders[1]
            .iter_mut()
            .for_each(|rl| rl.set_param("color", self.laser_colors[1]));
    }

    fn lua_game_state(
        &self,
        viewport: Viewport,
        camera: &Camera,
        hit_window: HitWindow,
        render_tick: u32,
    ) -> lua_data::LuaGameState {
        let screen = vec2(viewport.width as f32, viewport.height as f32);
        let track_center = graphics::camera_to_screen(camera, Vec3::zero(), screen);

        let track_left = graphics::camera_to_screen(camera, Vec3::unit_x() * -0.5, screen);
        let track_right = graphics::camera_to_screen(camera, Vec3::unit_x() * 0.5, screen);
        let crit_line = track_right - track_left;
        let rotation = -crit_line.y.atan2(crit_line.x);

        lua_data::LuaGameState {
            title: self.chart.meta.title.clone(),
            artist: self.chart.meta.artist.clone(),
            jacket_path: self.song.as_ref().difficulties.read().expect("Lock error")[self.diff_idx]
                .jacket_path
                .clone(),
            demo_mode: false,
            difficulty: self.chart.meta.difficulty,
            level: self.chart.meta.level,
            progress: self.current_time().as_secs_f32() / self.duration_secs,
            hispeed: self.view.hispeed,
            hispeed_adjust: 0,
            bpm: self.chart.bpm_at_tick(render_tick) as f32,
            gauge: lua_data::LuaGauge::from(&self.gauge.active),
            hidden_cutoff: 0.0,
            sudden_cutoff: 0.0,
            hidden_fade: 0.0,
            sudden_fade: 0.0,
            autoplay: self.autoplay.any(),
            combo_state: 0,
            note_held: [false; 6],
            laser_active: [self.laser_active[0], self.laser_active[1]],
            score_replays: Vec::new(),
            crit_line: lua_data::CritLine {
                x: track_center.x as i32,
                y: track_center.y as i32,
                x_offset: 0.0,
                rotation,
                cursors: [
                    lua_data::Cursor::new(
                        self.laser_cursors[0] as f32 * self.laser_wide[0] as f32
                            - (0.5 * (self.laser_wide[0].saturating_sub(1)) as f32),
                        camera,
                        if self.laser_target[0].is_some() {
                            1.0
                        } else {
                            0.0
                        },
                    ),
                    lua_data::Cursor::new(
                        self.laser_cursors[1] as f32 * self.laser_wide[1] as f32
                            - (0.5 * (self.laser_wide[1].saturating_sub(1)) as f32),
                        camera,
                        if self.laser_target[1].is_some() {
                            1.0
                        } else {
                            0.0
                        },
                    ),
                ],
                line: lua_data::Line {
                    x1: track_left.x,
                    y1: track_left.y,
                    x2: track_right.x,
                    y2: track_right.y,
                },
            },
            hit_window,
            multiplayer: false,
            user_id: "Player".into(),
            practice_setup: false,
        }
    }

    fn reset_canvas(&mut self) {
        let Some(vgfx) = self.lua.app_data_mut::<RefMut<Vgfx>>() else {
            log::error!("VGFX app data not set");
            return;
        };

        let vgfx = vgfx.write().expect("Lock error");
        let canvas = &mut vgfx.canvas.lock().expect("Lock error");
        canvas.flush();
        canvas.reset();
        canvas.reset_transform();
        canvas.reset_scissor();
    }

    fn on_hit(&mut self, hit_rating: HitRating) {
        self.hit_ratings.push(hit_rating);

        self.real_score += match hit_rating {
            HitRating::Crit { .. } => 2,
            HitRating::Good { .. } => 1,
            _ => 0,
        };

        let combo_updated = match hit_rating {
            HitRating::Crit { .. } | HitRating::Good { .. } => {
                self.combo += 1;
                self.max_combo = self.max_combo.max(self.combo);
                true
            }
            HitRating::Miss { .. } => {
                if self.combo == 0 {
                    false
                } else {
                    self.combo = 0;
                    true
                }
            }
            HitRating::None => false,
        };

        if combo_updated {
            if let Ok(update_combo) = self.lua.globals().get::<Function>("update_combo") {
                crate::log_result!(update_combo.call::<()>(self.combo));
            }
        }

        match hit_rating {
            HitRating::Crit {
                tick,
                delta,
                time: _,
            } => match tick.tick {
                ScoreTick::Chip { lane } => {
                    self.beam_colors_current[lane] = self.get_beam_color(lane, 2, delta);
                }
                ScoreTick::Slam { lane, start, end } => {
                    let laser_slam_hit = self.lua.globals().get::<Function>("laser_slam_hit");

                    let signum = (end - start).signum() as i32;
                    self.camera.shakes.push(CameraShake::new(
                        ((start - end).abs().powf(0.5) * 1.2).to_radians() as _,
                        signum as _,
                        20.0,
                        100.0,
                    ));

                    {
                        let events = &mut self.chart.camera.cam.pattern.laser.slam_event;

                        if let Ok(i) = events.half_spin.binary_search_by_key(&tick.y, |x| x.0) {
                            let cam_pattern_invoke_spin = events.half_spin[i];
                            if events.half_spin[i].1 == signum {
                                events.half_spin.remove(i);
                                self.camera
                                    .spins
                                    .push(camera::CameraSpin::Half(cam_pattern_invoke_spin));
                            }
                        }
                        if let Ok(i) = events.spin.binary_search_by_key(&tick.y, |x| x.0) {
                            let cam_pattern_invoke_spin = events.spin[i];
                            if cam_pattern_invoke_spin.1 == signum {
                                events.spin.remove(i);
                                self.camera
                                    .spins
                                    .push(camera::CameraSpin::Full(cam_pattern_invoke_spin));
                            }
                        }
                        if let Ok(i) = events.swing.binary_search_by_key(&tick.y, |x| x.0) {
                            let cam_pattern_invoke_swing = events.swing[i];
                            if cam_pattern_invoke_swing.1 == signum {
                                events.swing.remove(i);
                                self.camera
                                    .spins
                                    .push(camera::CameraSpin::Swing(cam_pattern_invoke_swing));
                            }
                        }
                    }

                    //TODO: Does this actually help?
                    self.laser_buffer[lane].clear();

                    if let Some(slam_sample) = self.slam_sample.clone() {
                        drop(std::mem::take(&mut self.slam_marker));
                        self.mixer.add(owned_source(
                            slam_sample.convert_samples().amplify(self.slam_volume),
                            &self.slam_marker,
                        )); //TODO: Amplyfy with slam volume
                    }

                    if let Ok(laser_slam_hit) = laser_slam_hit {
                        log_result!(laser_slam_hit.call::<()>((
                            end - start,
                            start - 0.5,
                            end - 0.5,
                            lane
                        )));
                    }
                }
                _ => (),
            },
            HitRating::Good {
                tick,
                delta,
                time: _,
            } => {
                if let ScoreTick::Chip { lane } = tick.tick {
                    self.beam_colors_current[lane] = self.get_beam_color(lane, 1, delta);
                    if let Ok(near_hit) = self.lua.globals().get::<Function>("near_hit") {
                        log_result!(near_hit.call::<()>(delta < 0.0));
                    }
                }
            }

            HitRating::Miss {
                tick,
                delta,
                time: _,
            } => {
                if let ScoreTick::Chip { lane } = tick.tick {
                    if delta.abs() > f64::EPSILON {
                        self.beam_colors_current[lane] = self.get_beam_color(lane, 0, 0.0);
                    }
                }
            }

            _ => {}
        }

        self.gauge.on_hit(hit_rating);
    }

    fn get_beam_color(&mut self, lane: usize, rating: usize, delta: f64) -> [f32; 4] {
        let button_hit = self.lua.globals().get::<Function>("button_hit");

        let mut beam_color: [f32; 4] = (self.beam_colors[rating] / 255.0).into();
        if let Ok(button_hit) = &button_hit {
            let (r, g, b) = button_hit
                .call::<(Option<u8>, Option<u8>, Option<u8>)>((lane, rating, delta))
                .inspect_err(|e| warn!("{e}"))
                .unwrap_or_default();

            if let (Some(r), Some(g), Some(b)) = (r, g, b) {
                beam_color[0] = r as f32 / 255.0;
                beam_color[1] = g as f32 / 255.0;
                beam_color[2] = b as f32 / 255.0;
            }
        }
        beam_color
    }

    pub const MAX_SCORE: u64 = 10_000_000_u64;
    fn actual_display_score(&self) -> u64 {
        let max = self.score_summary.total as u64 * 2;
        Self::MAX_SCORE * self.real_score / max
    }
    fn calculate_display_score(&self) -> u64 {
        let max = self.score_summary.total as u64 * 2;
        match self.score_display {
            ScoreDisplayMode::Additive => self.actual_display_score(),
            ScoreDisplayMode::Subtractive => {
                Self::MAX_SCORE * (max - (self.score_current_max - self.real_score)) / max
            }
            ScoreDisplayMode::Average => {
                Self::MAX_SCORE * self.real_score / self.score_current_max.max(1)
            }
        }
    }

    fn hold_ok(&self, lane: usize, start_tick: u32) -> bool {
        let is_button_held = &self.input_state.is_button_held((lane as u8).into());
        let start_ms = self.without_offset(self.chart.tick_to_ms(start_tick));
        let hold_start = self.zero_time + Duration::from_secs_f64(start_ms / 1000.0);
        let hold_start_thres = hold_start
            .checked_sub(self.hit_window.hold)
            .unwrap_or(hold_start);
        is_button_held.is_some_and(|t| t > hold_start_thres)
    }

    fn process_tick(
        &mut self,
        tick: PlacedScoreTick,
        chip_miss_tick: u32,
        slam_miss_tick: u32,
    ) -> HitRating {
        let time = self.current_time().as_secs_f64() * 1000.0;

        match tick.tick {
            ScoreTick::Hold { lane, start_tick } => {
                if self.hold_ok(lane, start_tick) || self.auto_buttons() {
                    HitRating::Crit {
                        tick,
                        delta: 0.0,
                        time,
                    }
                } else {
                    HitRating::Miss {
                        tick,
                        delta: 0.0,
                        time,
                    }
                }
            }
            ScoreTick::Laser { lane, pos } => {
                if (self.laser_cursors[lane] - pos).abs() < LASER_THRESHOLD || self.auto_lasers() {
                    HitRating::Crit {
                        tick,
                        delta: 0.0,
                        time,
                    }
                } else {
                    HitRating::Miss {
                        tick,
                        delta: 0.0,
                        time,
                    }
                }
            }
            ScoreTick::Slam { lane, start, end } => {
                assert!(end != start);
                let ms = self.chart.tick_to_ms(tick.y);
                let dir = match end.total_cmp(&start) {
                    Ordering::Less => 0,
                    Ordering::Greater => 1,
                    Ordering::Equal => unreachable!(),
                };
                let delta = ms
                    - self.with_offset(
                        self.laser_latest_dir_inputs[lane][dir]
                            .duration_since(self.zero_time)
                            .unwrap_or(Duration::ZERO)
                            .as_secs_f64()
                            * 1000.0,
                    );
                let contains_cursor = true; //TODO: (start.min(end)..=start.max(end)).contains(&self.laser_cursors[lane]);
                if tick.y < slam_miss_tick {
                    self.laser_assist_ticks[lane] = 0;
                    HitRating::Miss { tick, delta, time }
                } else if self.auto_lasers()
                    || (delta.abs() < (self.hit_window.slam.as_secs_f64() * 1000.0)
                        && contains_cursor)
                {
                    self.laser_cursors[lane] = end;
                    self.laser_assist_ticks[lane] = 24;
                    HitRating::Crit { tick, delta, time }
                } else {
                    HitRating::None
                }
            }
            ScoreTick::Chip { lane: _ } => {
                if tick.y < chip_miss_tick {
                    HitRating::Miss {
                        tick,
                        delta: 0.0,
                        time,
                    }
                } else if self.auto_buttons() {
                    HitRating::Crit {
                        tick,
                        delta: 0.0,
                        time,
                    }
                } else {
                    HitRating::None
                }
            }
        }
    }
    fn current_time(&self) -> std::time::Duration {
        if !self.intro_done {
            Duration::ZERO
        } else {
            SystemTime::now()
                .duration_since(self.zero_time)
                .unwrap_or(Duration::ZERO)
        }
    }

    fn with_offset(&self, time_ms: f64) -> f64 {
        time_ms
            - self.global_offset
            - self.chart.audio.bgm.offset as f64
            - self.playback.leadin().as_secs_f64() * 1000.0
    }

    fn without_offset(&self, time_ms: f64) -> f64 {
        time_ms
            + self.global_offset
            + self.chart.audio.bgm.offset as f64
            + self.playback.leadin().as_secs_f64() * 1000.0
    }

    fn fail_song(&mut self) -> anyhow::Result<()> {
        //TODO: Enter fail transition state
        self.transition_to_results()?;
        Ok(())
    }

    fn transition_to_results(&mut self) -> Result<(), anyhow::Error> {
        if let AutoPlay::None = self.autoplay {
            self.control_tx
                .as_ref()
                .ok_or(anyhow!("control_tx not set"))?
                .send(ControlMessage::Result {
                    song: self.song.clone(),
                    diff_idx: self.diff_idx,
                    score: self.actual_display_score() as u32,
                    gauge: std::mem::take(&mut self.gauge.active),
                    hit_ratings: std::mem::take(&mut self.hit_ratings),
                    autoplay: self.autoplay,
                    duration: (self.duration_secs * 1000.0) as i32,
                    hit_window: self.hit_window,
                    manual_exit: false,
                    max_combo: self.max_combo as _,
                    hash: self.chart.file_hash.clone(),
                })
                .expect("Main loop messaging error");
        } else {
            self.closed = true;
        }
        Ok(())
    }

    fn auto_buttons(&self) -> bool {
        matches!(self.autoplay, AutoPlay::All | AutoPlay::Buttons)
    }

    fn auto_lasers(&self) -> bool {
        matches!(self.autoplay, AutoPlay::All | AutoPlay::Lasers)
    }

    fn take_laser_input(&mut self, index: usize, now: SystemTime, tick: u32) -> bool {
        let Some((time_stamp, delta)) = self.laser_buffer[index].pop_front() else {
            return false;
        };

        let Ok(delay) = now.duration_since(time_stamp) else {
            return false;
        };

        if delay < self.laser_input_delay && self.laser_assist_ticks[index] > 0 {
            self.laser_buffer[index].push_front((time_stamp, delta));
            return false;
        }

        let input_dir = delta.total_cmp(&0.0);
        let delta = delta * 0.45;

        self.laser_cursors[index] = if self.laser_target[index].is_some() {
            let new_pos = (self.laser_cursors[index] + delta).clamp(0.0, 1.0);
            let target_value = self.chart.note.laser[index].value_at(tick as f64);

            //TODO: Not sure this is a good way to do laser offset but it might work
            let target_dir_offset = self.laser_offset / self.chart.tick_duration_ms_at(tick);
            let target_dir = self.chart.note.laser[index]
                .direction_at(tick as f64 + target_dir_offset)
                .map(|x| x.total_cmp(&0.0))
                .unwrap_or(std::cmp::Ordering::Equal);

            // overshooting logic
            let new_pos = if let Some(target_value) = target_value {
                match (
                    self.laser_cursors[index].total_cmp(&target_value),
                    new_pos.total_cmp(&target_value),
                    target_dir,
                ) {
                    (a, Ordering::Equal, b) if a != b => target_value,
                    (a, b, Ordering::Equal) if a != b => target_value,
                    (Ordering::Equal, a, b) if a == b => target_value,
                    (Ordering::Less, Ordering::Greater, Ordering::Less) => new_pos, // old \ new
                    (Ordering::Less, Ordering::Greater, Ordering::Greater) => target_value, // old / new
                    (Ordering::Greater, Ordering::Less, Ordering::Less) => target_value, // new \ old
                    (Ordering::Greater, Ordering::Less, Ordering::Greater) => new_pos, // new / old
                    (a, b, _) if a == b => new_pos,
                    _ => new_pos,
                }
            } else {
                new_pos
            };

            let on_laser = target_value
                .map(|v| (v - new_pos).abs() < LASER_THRESHOLD)
                .unwrap_or(false);

            if on_laser && input_dir == target_dir {
                self.laser_assist_ticks[index] = 20;
            }

            new_pos
        } else {
            0.0
        };

        true
    }

    fn get_hit_rating(
        &mut self,
        button: UscButton,
        button_num: u8,
        timestamp: SystemTime,
        perfect: Duration,
        good: Duration,
        miss: Duration,
    ) -> HitRating {
        let last_tick = self.chart.ms_to_tick(
            self.with_offset(self.current_time().as_secs_f64() * 1000.0)
                + miss.as_secs_f64() * 1000.0,
        ) + 1;
        let mut hittable_ticks = self.score_ticks.iter().take_while(|x| x.y < last_tick);
        let mut hit_rating = HitRating::None;
        match button {
            crate::button_codes::UscButton::BT(_) | crate::button_codes::UscButton::FX(_)
                if !self.auto_buttons() =>
            {
                if let Some((index, score_tick)) = hittable_ticks.find_position(|x| {
                    if let ScoreTick::Chip { lane } | ScoreTick::Hold { lane, .. } = x.tick {
                        lane == button_num as usize
                    } else {
                        false
                    }
                }) {
                    if let ScoreTick::Hold { .. } = score_tick.tick {
                        return hit_rating; // Next tick in this lane is a hold, do nothing
                    }
                    let tick = *score_tick;
                    let ms = self.chart.tick_to_ms(score_tick.y);
                    let time = self.with_offset(
                        timestamp
                            .duration_since(self.zero_time)
                            .unwrap_or(Duration::ZERO)
                            .as_secs_f64()
                            * 1000.0,
                    );

                    let delta = ms - time + self.button_offset;
                    let abs_delta = Duration::from_secs_f64(delta.abs() / 1000.0);

                    hit_rating = if abs_delta <= perfect {
                        HitRating::Crit { tick, delta, time }
                    } else if abs_delta <= good {
                        HitRating::Good { tick, delta, time }
                    } else if abs_delta <= miss {
                        HitRating::Miss { tick, delta, time }
                    } else {
                        HitRating::None
                    };

                    match hit_rating {
                        HitRating::None => {}
                        _ => {
                            self.on_hit(hit_rating);
                            self.score_ticks.remove(index);
                            self.score_current_max += 2;
                        }
                    }
                }
            }
            crate::button_codes::UscButton::Back => self.closed = true,
            _ => {}
        }
        hit_rating
    }
}

impl Scene for Game {
    fn closed(&self) -> bool {
        self.closed
    }
    fn render_ui(&mut self, _dt: f64) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_suspended(&self) -> bool {
        false
    }

    fn tick(&mut self, _dt: f64, _knob_state: crate::button_codes::LaserState) -> Result<()> {
        profile_function!();
        const AVG_DELTA_LEN: usize = 32;
        let mut time = self.current_time();
        let sys_time = SystemTime::now();

        let playback_ms = self.playback.get_ms();
        let timing_delta = playback_ms.sub(time.as_secs_f64() * 1000.0);
        let processing_tick = self
            .tick_queue
            .pop_front()
            .unwrap_or(self.duration_ticks + 10);
        if playback_ms > 0.0 {
            self.sync_delta.push_front(timing_delta);
            if self.sync_delta.len() > AVG_DELTA_LEN {
                self.sync_delta.pop_back();
            }
        }

        while self.take_laser_input(0, sys_time, processing_tick) {}
        while self.take_laser_input(1, sys_time, processing_tick) {}

        // Set roll despite chart not starting to set up correct start angle
        let keep_laser = match self
            .chart
            .camera
            .tilt
            .keep
            .binary_search_by_key(&processing_tick, |x| x.0)
        {
            Ok(i) => self.chart.camera.tilt.keep[i].1,
            Err(i) => {
                if i == 0 {
                    false
                } else {
                    self.chart.camera.tilt.keep[i.saturating_sub(1)].1
                }
            }
        };

        self.target_roll = self
            .chart
            .camera
            .tilt
            .manual
            .value_at(processing_tick as f64)
            .map(TargetRoll::Manual)
            .unwrap_or_else(|| {
                let current = self.target_roll;

                let next = match self.laser_target {
                    [Some(l), Some(r)] => TargetRoll::Laser(r + l - 1.0),
                    [Some(l), None] => TargetRoll::Laser(l),
                    [None, Some(r)] => TargetRoll::Laser(r - 1.0),
                    _ => TargetRoll::None,
                };

                if keep_laser {
                    match (current, next) {
                        (TargetRoll::Laser(v), TargetRoll::None)
                        | (TargetRoll::None, TargetRoll::Laser(v))
                        | (TargetRoll::Manual(v), TargetRoll::None) => TargetRoll::Laser(v),
                        (TargetRoll::Laser(cv), TargetRoll::Laser(nv))
                        | (TargetRoll::Manual(cv), TargetRoll::Laser(nv)) => {
                            if cv < f64::EPSILON {
                                TargetRoll::Laser(nv)
                            } else if cv.is_sign_negative() == nv.is_sign_negative() {
                                TargetRoll::Laser(cv.abs().max(nv.abs()) * cv.signum())
                            } else {
                                TargetRoll::Laser(cv)
                            }
                        }
                        _ => TargetRoll::None,
                    }
                } else {
                    next
                }
            });

        // Chart hasn't started, don't score anything yet
        if processing_tick == 0 && self.chart.ms_to_tick(self.with_offset(playback_ms)) == 0 {
            self.tick_queue.push_front(0);
            return Ok(());
        }

        let avg_delta: f64 = self.sync_delta.iter().fold(0.0, |a, c| a + c) / AVG_DELTA_LEN as f64;

        if playback_ms > 0.0 && !self.score_ticks.is_empty() {
            if avg_delta.abs() > 250.0 {
                self.sync_delta.clear();
                self.zero_time = SystemTime::now().sub(Duration::from_millis(playback_ms as _));
            } else if avg_delta.abs() > 1.0 {
                if avg_delta > 0.0 {
                    self.zero_time -= Duration::from_nanos(50000);
                } else {
                    self.zero_time += Duration::from_nanos(50000);
                }
            }

            time = self.current_time();
        }

        if self.tick_queue.is_empty() && !self.results_requested {
            self.transition_to_results()?;
            self.results_requested = true;
        }
        let missed_chip_tick = self.chart.ms_to_tick(
            self.with_offset(time.saturating_sub(self.hit_window.good).as_secs_f64() * 1000.0),
        );

        let auto_lasers = self.auto_lasers();

        for (side, ((laser_active, laser_target), wide)) in self
            .laser_active
            .iter_mut()
            .zip(self.laser_target.iter_mut())
            .zip(self.laser_wide.iter_mut())
            .enumerate()
        {
            let was_none = laser_target.is_none();
            *laser_target = self.chart.note.laser[side].value_at(processing_tick as f64);
            *wide = self.chart.note.laser[side].wide_at(processing_tick as f64);
            *laser_active = if let Some(val) = laser_target {
                (*val - self.laser_cursors[side]).abs() < LASER_THRESHOLD
            } else {
                false
            };

            if (was_none && laser_target.is_some()) || auto_lasers {
                self.laser_assist_ticks[side] = 10;
            }
            //TODO: Also check ahead
        }

        let laser_freq = match self.laser_target {
            [Some(l), Some(r)] => Some(r.mul_add(-1.0, 1.0).max(l)),
            [Some(l), None] => Some(l),
            [None, Some(r)] => Some(r.mul_add(-1.0, 1.0)),
            _ => None,
        };

        let laser_effect = self
            .laser_effects
            .range(0..=processing_tick)
            .rev()
            .map(|x| x.1)
            .next()
            .unwrap_or(&self.default_laser_effect);

        _ = if let Some((f, s)) =
            laser_freq.and_then(|x| laser_effect.get_biquad_state(x as _).map(|v| (x, v)))
        {
            self.biquad_control.send((
                Some(s),
                Some((1.0 - (f - 0.5).abs() * 1.99).powf(0.1) as f32),
            ))
        } else {
            self.biquad_control.send((None, Some(0.0)))
        };

        for (side, assist_ticks) in self.laser_assist_ticks.iter_mut().enumerate() {
            //TODO: If on straight laser, keep assist high
            let next_laser_is_slam = || {
                self.score_ticks
                    .iter()
                    .find(|t| match t.tick {
                        ScoreTick::Laser { lane, .. } => lane == side,
                        ScoreTick::Slam { lane, .. } => lane == side,
                        _ => false,
                    })
                    .map(|x| match x.tick {
                        ScoreTick::Slam { .. } => x.y,
                        _ => u32::MAX,
                    })
                    .unwrap_or(u32::MAX)
            };
            if *assist_ticks > 0 && processing_tick < next_laser_is_slam() {
                self.laser_cursors[side] = self.chart.note.laser[side]
                    .value_at(processing_tick as f64)
                    .unwrap_or(self.laser_cursors[side]);
            }
            *assist_ticks = assist_ticks.saturating_sub(1);
        }

        let mut i = 0;
        while i < self.score_ticks.len() {
            if self.score_ticks[i].y > processing_tick {
                break;
            }

            match self.process_tick(self.score_ticks[i], missed_chip_tick, missed_chip_tick) {
                HitRating::None => i += 1,
                r => {
                    self.on_hit(r);
                    self.score_ticks.remove(i);
                    self.score_current_max += 2;
                }
            }
        }

        self.playback.set_fx_enable(
            self.input_state
                .is_button_held(UscButton::FX(kson::Side::Left))
                .is_some()
                || self.auto_buttons(),
            self.input_state
                .is_button_held(UscButton::FX(kson::Side::Right))
                .is_some()
                || self.auto_buttons(),
        );

        self.camera.check_spins(processing_tick);

        self.gauge
            .update_sample(GAUGE_SAMPLES * processing_tick as usize / self.duration_ticks as usize);

        //Laser alerts
        if self.intro_done {
            let check_tick = (time.as_millis() + 1500) as f64;
            let check_tick = self.chart.ms_to_tick(self.with_offset(check_tick));

            //TODO: This can fire too many times
            for side in Side::iter() {
                let next_laser = match self.chart.note.laser[side as usize]
                    .binary_search_by_key(&check_tick, |x| x.0)
                {
                    Ok(x) | Err(x) => x,
                };

                if next_laser != self.laser_alert[side as usize]
                    && !self.chart.note.laser[side as usize].is_empty()
                {
                    if self.laser_target[side as usize].is_none() {
                        if let Ok(f) = self.lua.globals().get::<Function>("laser_alert") {
                            log_result!(f.call::<()>(side == Side::Right));
                        }
                    }
                    self.laser_alert[side as usize] = next_laser;
                }
            }
        }

        //Score display
        let display_score = self.calculate_display_score();
        if display_score != self.display_score {
            self.display_score = display_score;
            if let Ok(update_score) = self.lua.globals().get::<Function>("update_score") {
                crate::log_result!(update_score.call::<()>(display_score));
            }
        }

        if self.gauge.is_dead() {
            self.fail_song()?;
        }

        Ok(())
    }

    fn suspend(&mut self) {
        self.closed = true;
    }

    fn init(&mut self, app_control_tx: Sender<ControlMessage>) -> Result<()> {
        profile_function!();
        let lua_provider: Arc<LuaProvider> = self.service_provider.get_required();
        ensure!(self.score_summary.total != 0, "Empty chart");
        let long_count = self.score_summary.hold_count + self.score_summary.laser_count;
        let chip_count = self.score_summary.chip_count + self.score_summary.slam_count;
        let ftotal = 2.10 + f32::EPSILON;
        let (chip_gain, tick_gain) = if long_count == 0 && chip_count != 0 {
            (ftotal / chip_count as f32, 0.0f32)
        } else if long_count != 0 && chip_count == 0 {
            (0f32, ftotal / long_count as f32)
        } else {
            let gain = (ftotal * 20.0) / (5.0 * (long_count as f32 + (4.0 * chip_count as f32)));
            (gain, gain / 4.0)
        };

        let config = GameConfig::get();
        let fallbacks = (config.start_gauge.fallback_supported() && config.fallback_gauge)
            .then(|| GaugeType::Normal.get_gauge(chip_gain, tick_gain))
            .into_iter()
            .collect();
        self.gauge = Gauges::new(
            config.start_gauge.get_gauge(chip_gain, tick_gain),
            fallbacks,
        );
        self.control_tx = Some(app_control_tx);
        lua_provider.register_libraries(self.lua.clone(), "gameplay.lua")?;
        Ok(())
    }

    fn debug_ui(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        let current_tick = self
            .chart
            .ms_to_tick(self.current_time().as_secs_f64() * 1000.0);
        use egui::*;
        Window::new("Camera").show(ctx, |ui| {
            self.camera.egui_widget(ui);
            ui.checkbox(&mut self.draw_axis_guides, "Draw axies guides")
        });
        Window::new("Game Data")
            .scroll2([false, true])
            .show(ctx, |ui| {
                egui::Grid::new("gameplay_data")
                    .num_columns(2)
                    .spacing([40.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Time");
                        let mut current_sec = self.current_time().as_secs_f32();
                        if ui
                            .add(Slider::new(&mut current_sec, 0.0..=self.duration_secs))
                            .changed()
                        {
                            self.zero_time =
                                SystemTime::now().sub(Duration::from_secs_f32(current_sec))
                        }

                        ui.end_row();

                        ui.label("Sync delta (ms)");
                        let line: PlotPoints = self
                            .sync_delta
                            .iter()
                            .enumerate()
                            .map(|(x, y)| [x as f64, *y])
                            .collect();
                        let line = Line::new(line);
                        egui_plot::Plot::new("sync_delta").show(ui, |plot| {
                            plot.line(line);
                        });
                        ui.end_row();

                        ui.label("HiSpeed");
                        ui.add(Slider::new(&mut self.view.hispeed, 0.001..=2.0));

                        ui.end_row();
                        ui.separator();
                        ui.end_row();

                        ui.label("Note Data");
                        ui.end_row();

                        for i in 0..6 {
                            let mut next_tick = self
                                .score_ticks
                                .iter()
                                .filter(|x| x.y > current_tick)
                                .find(|x| match x.tick {
                                    ScoreTick::Chip { lane } | ScoreTick::Hold { lane, .. } => {
                                        lane == i
                                    }
                                    _ => false,
                                })
                                .map(|x| x.y)
                                .unwrap_or(u32::MAX)
                                .saturating_sub(current_tick);
                            ui.label(match i {
                                0 => "BT A",
                                1 => "BT B",
                                2 => "BT C",
                                3 => "BT D",
                                4 => "FX L",
                                5 => "FX R",
                                _ => unreachable!(),
                            });
                            ui.add(Slider::new(&mut next_tick, 0..=10000).logarithmic(true));
                            ui.end_row();
                        }

                        ui.label("Laser Values");
                        ui.end_row();

                        ui.label("Left");

                        if let Some(mut lval) =
                            self.chart.note.laser[0].value_at(current_tick as f64)
                        {
                            ui.add(egui::Slider::new(&mut lval, 0.0..=1.0));
                        }

                        ui.end_row();

                        ui.label("Right");
                        if let Some(mut rval) =
                            self.chart.note.laser[1].value_at(current_tick as f64)
                        {
                            ui.add(egui::Slider::new(&mut rval, 0.0..=1.0));
                        }
                        ui.end_row();

                        ui.label("Laser Direction");
                        ui.end_row();

                        ui.label("Left");
                        ui.label(format!(
                            "{:?}",
                            self.chart.note.laser[0]
                                .direction_at(current_tick as f64)
                                .map(|x| x.total_cmp(&0.0))
                        ));
                        ui.end_row();

                        ui.label("Right");
                        ui.label(format!(
                            "{:?}",
                            self.chart.note.laser[1]
                                .direction_at(current_tick as f64)
                                .map(|x| x.total_cmp(&0.0))
                        ));
                        ui.end_row();

                        ui.label("Stats");
                        ui.add(
                            egui::Label::new(format!("{:#?}", &self.beam_colors_current))
                                .wrap_mode(TextWrapMode::Extend),
                        )
                    });
            });

        Ok(())
    }

    fn render(
        &mut self,
        dt: f64,
        td_context: &three_d::Context,
        target: &mut three_d::RenderTarget,
        viewport: Viewport,
    ) {
        profile_function!();

        self.camera
            .update(vec2(viewport.width as f32, viewport.height as f32));
        if self.intro_done && !self.playback.is_playing() {
            info!("Starting playback");
            self.zero_time = SystemTime::now();
            if !self.playback.play() {
                log::error!("Could not play audio");
                self.closed = true;
                return;
            };

            let (biquad_control, biquad_events) = std::sync::mpsc::channel();

            self.biquad_control = biquad_control;

            self.mixer.add(owned_source(
                biquad(
                    self.playback.get_source().expect("Audio not loaded"),
                    BiQuadState::new(BiQuadType::AllPass, SQRT_2, 100.0),
                    Some(biquad_events),
                ),
                &self.source_owner,
            ));
        }

        let leadin_ms = self.playback.get_ms().min(0.0);

        let time = self.current_time();
        let time_ms = time.as_secs_f64() * 1000.0 + leadin_ms;

        self.view.cursor = self.with_offset(time.as_secs_f64() * 1000.0);
        let render_tick = self.chart.ms_to_tick(self.view.cursor);

        //Update roll
        {
            profile_scope!("Update camera");
            let max_roll_speed = dt / kson::beat_in_ms(self.chart.bpm_at_tick(render_tick));
            self.current_roll = match self.target_roll {
                TargetRoll::Laser(target_roll) => {
                    let scale = match self
                        .chart
                        .camera
                        .tilt
                        .scale
                        .binary_search_by_key(&render_tick, |x| x.0)
                    {
                        Ok(i) => self.chart.camera.tilt.scale[i].1,
                        Err(i) => {
                            if i == 0 {
                                1.0
                            } else {
                                self.chart.camera.tilt.scale[i.saturating_sub(1)].1
                            }
                        }
                    };
                    let target_roll = target_roll * scale;
                    if self.current_roll - target_roll < 0.0 {
                        (self.current_roll + max_roll_speed * 2.0 * scale).min(target_roll)
                    } else {
                        (self.current_roll - max_roll_speed * 2.0 * scale).max(target_roll)
                    }
                }
                TargetRoll::Manual(v) => v,

                TargetRoll::None => {
                    if self.current_roll.is_sign_negative() {
                        (self.current_roll + max_roll_speed).min(0.0)
                    } else {
                        (self.current_roll - max_roll_speed).max(0.0)
                    }
                }
            };

            self.camera.tilt.0 = self.current_roll as f32 * 12.5;
            self.camera.tilt.1 = self
                .camera
                .spins
                .iter()
                .map(|x| x.roll_at(render_tick as f32))
                .sum::<f32>();
            self.camera.kson_radius =
                self.chart.camera.cam.body.zoom.value_at(render_tick as f64) as f32;
            self.camera.kson_angle = self
                .chart
                .camera
                .cam
                .body
                .rotation_x
                .value_at(render_tick as f64) as f32;

            self.camera.shakes.retain_mut(|x| {
                x.tick(dt as _);
                !x.completed()
            });
        }
        let td_camera: Camera = Camera::from(&self.camera);
        if let Some(bg) = self.background.as_mut() {
            bg.set_global("gameplay", &self.lua_game_state);
            bg.render(
                dt,
                &td_camera,
                time_ms,
                &self.chart,
                render_tick,
                self.camera.tilt,
                self.gauge.is_cleared(),
            );
        }

        self.beam_colors_current
            .iter_mut()
            .for_each(|c| c[3] = (c[3] - dt as f32 / 200.0).max(0.0));

        let new_lua_state = self.lua_game_state(viewport, &td_camera, self.hit_window, render_tick);
        if new_lua_state != self.lua_game_state {
            self.lua_game_state = new_lua_state;
            let lua_game_state = match self.lua.to_value(&self.lua_game_state) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("{e}");
                    return;
                }
            };
            log_result!(self.lua.globals().set("gameplay", lua_game_state));
        }

        //Set glow/hit states
        let object_glow = ((time_ms as f32 % 100.0) / 50.0 - 1.0).abs() * 0.5 + 0.5;
        let hit_state = (time_ms / 50.0).rem_euclid(2.0) as i32 + 2;
        for (side, [_, shader]) in self.laser_shaders.iter_mut().enumerate() {
            shader.set_param(
                "hitState",
                if self.laser_active[side] {
                    hit_state
                } else {
                    0
                },
            );

            shader.set_param(
                "objectGlow",
                if self.laser_active[side] {
                    object_glow
                } else {
                    0.3
                },
            );
        }

        self.track_shader.draw_camera(&td_camera);

        let render_data = match self.view.render(
            &self.chart,
            td_context,
            |lane, tick| self.hold_ok(lane, tick),
            self.beam_colors_current,
            self.chip_h,
        ) {
            Ok(d) => d,
            Err(e) => {
                log::error!("{e}");
                return;
            }
        };

        self.fx_long_shaders.draw_instanced_camera(
            &td_camera,
            render_data.fx_hold,
            |material, transform, (hold, active)| {
                material.use_uniform("world", transform * hold);
                let (glow, state) = match active {
                    HoldState::Idle => (0.6, 1),
                    HoldState::Hit => (object_glow, hit_state), //FLIP_Y?
                    HoldState::Miss => (0.3, 0),
                };
                material.use_uniform_if_required("objectGlow", glow);
                material.use_uniform_if_required("hitState", state);
            },
        );

        self.bt_long_shaders.draw_instanced_camera(
            &td_camera,
            render_data.bt_hold,
            |material, transform, (bt, active)| {
                material.use_uniform("world", transform * bt);
                let (glow, state) = match active {
                    HoldState::Idle => (0.6, 1),
                    HoldState::Hit => (object_glow, hit_state),
                    HoldState::Miss => (0.3, 0),
                };
                material.use_uniform_if_required("objectGlow", glow);
                material.use_uniform_if_required("hitState", state);
            },
        );

        self.fx_chip_shaders.draw_instanced_camera(
            &td_camera,
            render_data.fx_chip,
            |material, transform, (fx, has_sample)| {
                material.use_uniform("world", transform * fx);
                material.use_uniform_if_required("hasSample", if has_sample { 1 } else { 0 });
            },
        );

        self.bt_chip_shader.draw_instanced_camera(
            &td_camera,
            render_data.bt_chip,
            |material, transform, bt| material.use_uniform("world", transform * bt),
        );

        self.lane_beam_shader.draw_instanced_camera(
            &td_camera,
            render_data.lane_beams,
            |material, tranform, (light, color)| {
                material.use_uniform_if_required::<Vec4>("color", color.into());
                material.use_uniform("world", tranform * light);
            },
        );

        self.laser_shaders[0][0].set_data_mesh(&render_data.lasers[0]);
        self.laser_shaders[0][1].set_data_mesh(&render_data.lasers[1]);
        self.laser_shaders[1][0].set_data_mesh(&render_data.lasers[2]);
        self.laser_shaders[1][1].set_data_mesh(&render_data.lasers[3]);

        for ele in self.laser_shaders.iter().flatten() {
            ele.draw_camera(&td_camera);
        }

        if !self.intro_done {
            if let Ok(func) = self.lua.globals().get::<Function>("render_intro") {
                profile_scope!("lua render_intro");
                match func.call::<bool>(dt / 1000.0) {
                    Err(e) => {
                        log::error!("{}", e);
                    }
                    Ok(intro_complete) => self.intro_done = intro_complete,
                };
            }
        }

        if let Ok(func) = self.lua.globals().get::<Function>("render_crit_base") {
            profile_scope!("lua render_crit_base");
            if let Err(e) = func.call::<()>(dt / 1000.0) {
                log::error!("{}", e);
            };
        }
        self.reset_canvas();

        if let Some(fg) = self.foreground.as_mut() {
            fg.set_global("gameplay", &self.lua_game_state);
            fg.render(
                dt,
                &td_camera,
                time_ms,
                &self.chart,
                render_tick,
                self.camera.tilt,
                self.gauge.is_cleared(),
            );
        }

        if let Ok(func) = self.lua.globals().get::<Function>("render_crit_overlay") {
            profile_scope!("lua render_crit_overlay");
            if let Err(e) = func.call::<()>(dt / 1000.0) {
                log::error!("{}", e);
            };
        }
        self.reset_canvas();

        if let Ok(func) = self.lua.globals().get::<Function>("render") {
            profile_scope!("lua render");
            if let Err(e) = func.call::<()>(dt / 1000.0) {
                log::error!("{}", e);
            };
        }
        self.reset_canvas();
        if self.draw_axis_guides {
            let axes = three_d::Axes::new(td_context, 0.01, 0.30);
            target.render(&td_camera, [axes], &[]);
        }
    }

    fn on_event(&mut self, event: &winit::event::Event<crate::button_codes::UscInputEvent>) {
        if let winit::event::Event::UserEvent(UscInputEvent::Laser(ls, timestamp)) = event {
            //TODO: Slam detection, or always handle slam ticks in ticking function?

            for (side, index) in [(kson::Side::Left, 0), (kson::Side::Right, 1)] {
                let delta = ls.get_axis(side).delta as f64;

                if self.input_state.is_button_held(UscButton::Start).is_some() {
                    let mut config = GameConfig::get_mut();
                    self.view.hispeed += delta as f32 * 0.1;
                    self.view.hispeed = self.view.hispeed.clamp(0.1, 10.0);

                    config.mod_speed = (self.view.hispeed * self.lua_game_state.bpm) as f64;
                }

                let input_dir = delta.total_cmp(&0.0);
                match input_dir {
                    Ordering::Less => self.laser_latest_dir_inputs[index][0] = *timestamp,
                    Ordering::Equal => {}
                    Ordering::Greater => self.laser_latest_dir_inputs[index][1] = *timestamp,
                }

                if delta.abs() > 0.0 {
                    self.laser_buffer[index].push_back((*timestamp, delta));
                }
            }
        }
    }

    fn on_button_pressed(&mut self, button: crate::button_codes::UscButton, timestamp: SystemTime) {
        let HitWindow {
            variant: _,
            perfect,
            good,
            hold: _,
            miss,
            slam: _,
        } = self.hit_window;

        let button_num = Into::<u8>::into(button);

        let hit_rating = self.get_hit_rating(button, button_num, timestamp, perfect, good, miss);
        if let HitRating::None = hit_rating {
            if (button_num as usize) < self.beam_colors_current.len() {
                self.beam_colors_current[button_num as usize] =
                    self.get_beam_color(button_num as usize, 3, 0.0);
            }
        }
    }

    fn name(&self) -> &str {
        "Game"
    }
}

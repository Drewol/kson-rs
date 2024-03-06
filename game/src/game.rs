use crate::{
    button_codes::{UscButton, UscInputEvent},
    input_state::InputState,
    log_result,
    lua_service::LuaProvider,
    scene::{Scene, SceneData},
    shaded_mesh::ShadedMesh,
    songselect::Song,
    sources::{
        self,
        biquad::{biquad, BiQuadState, BiQuadType, BiquadController},
        bitcrush::bit_crusher,
        effected_part::effected_part,
        flanger::flanger,
        gate::gate,
        mix_source::{MixSource, NoMix},
        owned_source::owned_source,
        phaser::phaser,
        pitch_shift::pitch_shift,
        re_trigger::re_trigger,
        side_chain::side_chain,
        tape_stop::tape_stop,
        wobble::wobble,
    },
    vg_ui::Vgfx,
    ControlMessage,
};
use anyhow::{ensure, Result};
use di::{RefMut, ServiceProvider};
use egui_plot::{Line, PlotPoint, PlotPoints};
use image::GenericImageView;
use itertools::Itertools;
use kson::{
    effects::EffectInterval,
    score_ticks::{PlacedScoreTick, ScoreTick, ScoreTickSummary, ScoreTicker},
    Chart, Graph, Interval, Side, Track,
};
use log::info;
use puffin::{profile_function, profile_scope};
use rodio::{dynamic_mixer::DynamicMixerController, source::Buffered, Decoder, Source};
use std::{
    cmp::Ordering,
    collections::VecDeque,
    f32::consts::SQRT_2,
    ops::{Add, Sub},
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicI32, AtomicU16},
        mpsc::{Receiver, Sender},
        Arc,
    },
    time::{Duration, SystemTime},
};
use tealr::mlu::mlua::{Function, Lua, LuaSerdeExt};
use three_d::{vec2, vec3, Blend, Camera, Mat4, Matrix4, Vec3, Vec4, Viewport, Zero};
use three_d_asset::{vec4, Srgba};

mod chart_view;
use chart_view::*;
mod camera;
use camera::*;
mod background;
use background::GameBackground;
mod lua_data;
pub use lua_data::HitWindow;

const LASER_THRESHOLD: f64 = 1.0 / 12.0;
const LEADIN: Duration = Duration::from_secs(3);

pub struct Game {
    view: ChartView,
    chart: kson::Chart,
    zero_time: SystemTime,
    duration: u32,
    fx_long_shaders: ShadedMesh,
    bt_long_shaders: ShadedMesh,
    fx_chip_shaders: ShadedMesh,
    laser_shaders: [[ShadedMesh; 2]; 2], //[[left, left_current], [right, right_current]]
    track_shader: ShadedMesh,
    bt_chip_shader: ShadedMesh,
    lane_beam_shader: ShadedMesh,
    camera: ChartCamera,
    lua_game_state: lua_data::LuaGameState,
    lua: Rc<Lua>,
    intro_done: bool,
    song: Arc<Song>,
    diff_idx: usize,
    control_tx: Option<Sender<ControlMessage>>,
    gauge: Gauge,
    results_requested: bool,
    closed: bool,
    playback: kson_music_playback::AudioPlayback,
    score_ticks: Vec<PlacedScoreTick>,
    score_summary: ScoreTickSummary,
    real_score: u64,
    combo: u64,
    current_tick: u32,
    input_state: InputState,
    laser_cursors: [f64; 2],
    laser_active: [bool; 2],
    laser_wide: [u32; 2],
    laser_target: [Option<f64>; 2],
    laser_assist_ticks: [u8; 2],
    laser_latest_dir_inputs: [[SystemTime; 2]; 2], //last left/right turn timestamps for both knobs, for checking slam hits
    beam_colors: Vec<Vec4>,
    beam_colors_current: [[f32; 4]; 6],
    draw_axis_guides: bool,
    target_roll: Option<f64>,
    current_roll: f64,
    hit_ratings: Vec<HitRating>,
    mixer: Arc<DynamicMixerController<f32>>,
    biquad_control: BiquadController,
    source_owner: (Sender<()>, Receiver<()>),
    slam_sample: Option<Buffered<Decoder<std::fs::File>>>,
    background: Option<GameBackground>,
    foreground: Option<GameBackground>,
    service_provider: ServiceProvider,
    sync_delta: VecDeque<f64>,
}

#[derive(Debug, Default)]
enum Gauge {
    #[default]
    None,
    Normal {
        chip_gain: f32,
        tick_gain: f32,
        value: f32,
    },
}

fn tick_is_short(score_tick: PlacedScoreTick) -> bool {
    match score_tick.tick {
        ScoreTick::Laser { lane: _, pos: _ } => false,
        ScoreTick::Slam {
            lane: _,
            start: _,
            end: _,
        } => true,
        ScoreTick::Chip { lane: _ } => true,
        ScoreTick::Hold { lane: _ } => false,
    }
}

impl Gauge {
    pub fn on_hit(&mut self, rating: HitRating) {
        match self {
            Gauge::None => {}
            Gauge::Normal {
                chip_gain,
                tick_gain,
                value,
            } => match rating {
                HitRating::Crit {
                    tick: t,
                    delta: _,
                    time: _,
                } if tick_is_short(t) => *value += *chip_gain,
                HitRating::Crit {
                    tick: _,
                    delta: _,
                    time: _,
                } => *value += *tick_gain,
                HitRating::Good {
                    tick: _,
                    delta: _,
                    time: _,
                } => *value += *chip_gain / 3.0, //Only chips can have a "good" rating
                HitRating::Miss {
                    tick: t,
                    delta: _,
                    time: _,
                } if tick_is_short(t) => *value -= 0.02,
                HitRating::Miss {
                    tick: _,
                    delta: _,
                    time: _,
                } => *value -= 0.02 / 4.0,
                HitRating::None => {}
            },
        }

        //Clamp
        match self {
            Gauge::None => todo!(),
            Gauge::Normal {
                chip_gain: _,
                tick_gain: _,
                value,
            } => *value = value.clamp(0.0, 1.0),
        }
    }

    pub fn is_cleared(&self) -> bool {
        match self {
            Gauge::Normal {
                chip_gain: _,
                tick_gain: _,
                value,
            } => *value >= 0.7,
            Gauge::None => false,
        }
    }

    pub fn value(&self) -> f32 {
        match self {
            Gauge::None => 0.0,
            Gauge::Normal {
                chip_gain: _,
                tick_gain: _,
                value,
            } => *value,
        }
    }
}

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

impl From<&Gauge> for lua_data::LuaGauge {
    fn from(value: &Gauge) -> Self {
        match value {
            Gauge::Normal {
                chip_gain: _,
                tick_gain: _,
                value,
            } => lua_data::LuaGauge {
                gauge_type: 0,
                options: 0,
                value: *value,
                name: "Normal".into(),
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

enum HoldState {
    Idle,
    Hit,
    Miss,
}

mod graphics;
use graphics::*;

type ChartSource =
    std::boxed::Box<(dyn rodio::source::Source<Item = f32> + std::marker::Send + 'static)>;

pub struct GameData {
    song: Arc<Song>,
    diff_idx: usize,
    chart: kson::Chart,
    skin_folder: PathBuf,
    audio: std::boxed::Box<(dyn rodio::source::Source<Item = f32> + std::marker::Send + 'static)>,
    effect_audio:
        std::boxed::Box<(dyn rodio::source::Source<Item = f32> + std::marker::Send + 'static)>,
}

impl GameData {
    pub fn new(
        song: Arc<Song>,
        diff_idx: usize,
        chart: kson::Chart,
        skin_folder: PathBuf,
        audio: Box<dyn Source<Item = f32> + Send>,
    ) -> anyhow::Result<Self> {
        let audio = audio.buffered();
        let offset = Duration::from_millis(chart.audio.bgm.as_ref().unwrap().offset.max(0) as _);
        let neg_offset =
            Duration::from_millis(chart.audio.bgm.as_ref().unwrap().offset.min(0).abs() as _);
        //TODO: Does not belong in game crate
        //TODO: Sort effects for proper overlapping sounds
        //TODO: Effects are added quickly now but render slowly as most the effects run for the whole song even when mixed to 0
        let effect_audio: Box<dyn Source<Item = f32> + Send> = chart
            .get_effect_tracks()
            .iter()
            .fold(Box::new(audio.clone()), |current, effect| {
                let base = current;
                let start = chart.tick_to_ms(effect.interval.y);
                let end = chart.tick_to_ms(effect.interval.y + effect.interval.l);
                let end = Duration::from_nanos((end * 1000000.0) as _);
                let start = Duration::from_nanos((start * 1000000.0) as _);

                let start = start.add(offset).saturating_sub(neg_offset);
                let end = end.add(offset).saturating_sub(neg_offset);

                info!(
                    "Effecting part: {:?} - {:?} with {}",
                    &start,
                    &end,
                    effect.effect.name()
                );

                let effected: Box<dyn MixSource<Item = f32> + Send> = match &effect.effect {
                    kson::effects::AudioEffect::ReTrigger(r) => {
                        let duration = Duration::from_secs_f64(
                            (240.0 * r.wave_length.interpolate(1.0, true) as f64)
                                / chart.bpm_at_tick(effect.interval.y),
                        );

                        let update_duration = Duration::from_secs_f64(
                            (240.0 * r.update_period.interpolate(1.0, true) as f64)
                                / chart.bpm_at_tick(effect.interval.y),
                        );
                        Box::new(re_trigger(base, start, duration, update_duration, 1.0))
                    }
                    kson::effects::AudioEffect::Gate(g) => {
                        let period = Duration::from_secs_f64(
                            (240.0 * g.wave_length.interpolate(1.0, true) as f64)
                                / chart.bpm_at_tick(effect.interval.y),
                        );
                        Box::new(gate(base, start, period, 0.6, 0.4))
                    }
                    kson::effects::AudioEffect::Flanger(f) => Box::new(flanger(
                        base,
                        Duration::from_millis(4),
                        Duration::from_millis(1),
                        0.5,
                        0.05,
                    )),
                    kson::effects::AudioEffect::PitchShift(p) => {
                        Box::new(pitch_shift(base, p.pitch.interpolate(1.0, true) as _))
                    }
                    kson::effects::AudioEffect::BitCrusher(b) => {
                        Box::new(bit_crusher(base, b.reduction.interpolate(1.0, true) as _))
                    }
                    kson::effects::AudioEffect::Phaser(p) => Box::new(
                        //TODO
                        flanger(
                            base,
                            Duration::from_millis(4),
                            Duration::from_millis(1),
                            0.5,
                            0.05,
                        ),
                    ),
                    kson::effects::AudioEffect::Wobble(w) => Box::new(wobble(
                        base,
                        1.0 / ((240.0 * w.wave_length.interpolate(1.0, true))
                            / chart.bpm_at_tick(effect.interval.y) as f32),
                        w.lo_freq.interpolate(1.0, true) as _,
                        w.hi_freq.interpolate(1.0, true) as _,
                    )),
                    kson::effects::AudioEffect::TapeStop(t) => {
                        Box::new(tape_stop(base, start, end - start))
                    }
                    kson::effects::AudioEffect::Echo(r) => {
                        let duration = Duration::from_secs_f64(
                            (240.0 * r.wave_length.interpolate(1.0, true) as f64)
                                / chart.bpm_at_tick(effect.interval.y),
                        );
                        let feedback = r.feedback_level.interpolate(1.0, true).clamp(0.0, 1.0);

                        Box::new(re_trigger(base, start, duration, Duration::ZERO, feedback))
                    }
                    kson::effects::AudioEffect::SideChain(s) => {
                        let bpm = chart.bpm_at_tick(effect.interval.y) as f32;

                        Box::new(side_chain(
                            base,
                            start,
                            s.period.to_duration(bpm, 1.0, true),
                            s.attack_time.to_duration(bpm, 1.0, true),
                            s.hold_time.to_duration(bpm, 1.0, true),
                            s.release_time.to_duration(bpm, 1.0, true),
                            s.ratio.interpolate(1.0, true),
                        ))
                    }
                    _ => Box::new(NoMix(base)),
                };

                Box::new(effected_part(effected, start, end - start, 1.0))
            });

        let effect_audio = effect_audio.buffered();
        let renderer = effect_audio.clone();
        let total_duration = audio.total_duration().unwrap_or_else(|| {
            Duration::from_secs_f64(chart.tick_to_ms(chart.get_last_tick()) / 1000.0)
        });

        let redered_progress = Arc::new(AtomicU16::new(0));
        //Render effect audio here since we're on a different thread
        _ = renderer
            .periodic_access(total_duration / 100, move |_| {
                info!(
                    "Effects rendering: {}%",
                    redered_progress
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                        .min(100)
                )
            })
            .skip_duration(Duration::MAX);

        Ok(Self {
            chart,
            skin_folder,
            diff_idx,
            song,
            audio: Box::new(audio),
            effect_audio: Box::new(effect_audio),
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
            effect_audio,
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
        )?;

        beam_shader.set_data_mesh(&graphics::xy_rect(Vec3::zero(), vec2(1.0, 1.0)));

        fx_long_shader.use_texture(
            "mainTex",
            texture_folder.with_file_name("fxbuttonhold.png"),
            (false, false),
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
        )?;
        let fx_height = 1.0 / 12.0;

        fx_chip_shader.set_data_mesh(&graphics::xy_rect(
            vec3(0.0, fx_height / 2.0, 0.0),
            vec2(2.0 / 6.0, fx_height),
        ));

        let mut bt_chip_shader = ShadedMesh::new(&context, "button", &shader_folder)
            .expect("Failed to load shader:")
            .with_transform(Matrix4::from_translation(vec3(-0.5, 0.0, 0.0)));
        let bt_height = 1.0 / 12.0;
        bt_chip_shader.set_data_mesh(&graphics::xy_rect(
            vec3(0.0, bt_height / 2.0, 0.0),
            vec2(1.0 / 6.0, bt_height),
        ));

        bt_chip_shader.use_texture(
            "mainTex",
            texture_folder.with_file_name("button.png"),
            (false, false),
        )?;

        let mut track_shader =
            ShadedMesh::new(&context, "track", &shader_folder).expect("Failed to load shader:");
        track_shader.set_data_mesh(&graphics::xy_rect(
            Vec3::zero(),
            vec2(1.0, ChartView::TRACK_LENGTH * 2.0),
        ));

        track_shader.set_param("lCol", Srgba::BLUE);
        track_shader.set_param("rCol", Srgba::RED);

        track_shader.use_texture(
            "mainTex",
            texture_folder.with_file_name("track.png"),
            (false, false),
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
        )?;
        laser_left_active.use_texture(
            "mainTex",
            texture_folder.with_file_name("laser_l.png"),
            (false, true),
        )?;
        laser_right.use_texture(
            "mainTex",
            texture_folder.with_file_name("laser_r.png"),
            (false, true),
        )?;
        laser_right_active.use_texture(
            "mainTex",
            texture_folder.with_file_name("laser_r.png"),
            (false, true),
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
            .open(audio, "Game", Some(effect_audio))
            .expect("Failed to load audio");
        playback.build_effects(&chart);
        playback.stop();

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

        let background = match GameBackground::new(
            &context,
            true,
            &bg_folder,
            &chart,
            service_provider.get_required(),
            service_provider.get_required(),
        )
        .or_else(|e| {
            log::warn!("Failed to load background: {e} \n {:?}", &bg_folder);
            GameBackground::new(
                &context,
                true,
                bg_folder.with_file_name("fallback"),
                &chart,
                service_provider.get_required(),
                service_provider.get_required(),
            )
        }) {
            Ok(bg) => {
                log::info!("Background loaded");
                Some(bg)
            }
            Err(e) => {
                log::warn!(
                    "Failed to load fallback background: {e} \n {:?}",
                    &bg_folder.with_file_name("fallback")
                );
                None
            }
        };

        let foreground = GameBackground::new(
            &context,
            false,
            bg_folder,
            &chart,
            service_provider.get_required(),
            service_provider.get_required(),
        )
        .ok();

        Ok(Box::new(
            Game::new(
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
            )
            .unwrap(),
        ))
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
    ) -> Result<Self> {
        let mut view = ChartView::new(skin_root, td);
        view.build_laser_meshes(&chart);
        let duration = chart.ms_to_tick(3000.0 + chart.tick_to_ms(chart.get_last_tick()));
        let mut slam_path = skin_root.clone();
        slam_path.push("audio");
        slam_path.push("laser_slam.wav");

        let score_ticks = kson::score_ticks::generate_score_ticks(&chart);

        let mut res = Self {
            song,
            diff_idx,
            intro_done: false,
            lua: Rc::new(Lua::new()),
            chart,
            view,
            duration,
            zero_time: SystemTime::now(),
            bt_chip_shader,
            track_shader,
            bt_long_shaders,
            fx_chip_shaders,
            fx_long_shaders,
            laser_shaders,
            lane_beam_shader,
            camera: ChartCamera {
                fov: 90.0,
                radius: 1.1,
                angle: 130.0,
                center: Vec3::zero(),
                track_length: ChartView::TRACK_LENGTH,
                tilt: 0.0,
                view_size: vec2(0.0, 0.0),
                shakes: vec![],
            },
            lua_game_state: lua_data::LuaGameState::default(),
            control_tx: None,
            results_requested: false,
            closed: false,
            playback,
            score_summary: score_ticks.summary(),
            score_ticks,
            gauge: Gauge::default(),
            real_score: 0,
            combo: 0,
            current_tick: 0,
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
            draw_axis_guides: false,
            current_roll: 0.0,
            target_roll: None,
            hit_ratings: Vec::new(),
            mixer: service_provider.get_required(),
            biquad_control,
            background,
            foreground,
            source_owner: std::sync::mpsc::channel(),
            slam_sample: std::fs::File::open(slam_path)
                .ok()
                .and_then(|x| Decoder::new(x).ok())
                .map(|x| x.buffered()),
            service_provider,
            sync_delta: Default::default(),
            laser_wide: [1, 1],
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
            .for_each(|ll| ll.set_param("color", Srgba::BLUE));
        self.laser_shaders[1]
            .iter_mut()
            .for_each(|rl| rl.set_param("color", Srgba::RED));
    }

    fn lua_game_state(&self, viewport: Viewport, camera: &Camera) -> lua_data::LuaGameState {
        let screen = vec2(viewport.width as f32, viewport.height as f32);
        let track_center = graphics::camera_to_screen(camera, Vec3::zero(), screen);

        let track_left = graphics::camera_to_screen(camera, Vec3::unit_x() * -0.5, screen);
        let track_right = graphics::camera_to_screen(camera, Vec3::unit_x() * 0.5, screen);
        let crit_line = track_right - track_left;
        let rotation = -crit_line.y.atan2(crit_line.x);

        lua_data::LuaGameState {
            title: self.chart.meta.title.clone(),
            artist: self.chart.meta.artist.clone(),
            jacket_path: self.song.as_ref().difficulties.read().unwrap()[self.diff_idx]
                .jacket_path
                .clone(),
            demo_mode: false,
            difficulty: self.chart.meta.difficulty,
            level: self.chart.meta.level,
            progress: self.current_tick as f32 / self.chart.get_last_tick() as f32,
            hispeed: self.view.hispeed,
            hispeed_adjust: 0,
            bpm: self.chart.bpm_at_tick(self.current_tick) as f32,
            gauge: lua_data::LuaGauge::from(&self.gauge),
            hidden_cutoff: 0.0,
            sudden_cutoff: 0.0,
            hidden_fade: 0.0,
            sudden_fade: 0.0,
            autoplay: false,
            combo_state: 0,
            note_held: [false; 6],
            laser_active: [false; 2],
            score_replays: Vec::new(),
            crit_line: lua_data::CritLine {
                x: track_center.x as i32,
                y: track_center.y as i32,
                x_offset: 0.0,
                rotation,
                cursors: [
                    lua_data::Cursor::new(
                        self.laser_cursors[0] as f32 * self.laser_wide[0] as f32
                            - (0.5 * (self.laser_wide[0] - 1) as f32),
                        camera,
                        if self.laser_target[0].is_some() {
                            1.0
                        } else {
                            0.0
                        },
                    ),
                    lua_data::Cursor::new(
                        self.laser_cursors[1] as f32 * self.laser_wide[1] as f32
                            - (0.5 * (self.laser_wide[1] - 1) as f32),
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
            hit_window: HitWindow {
                variant: 1,
                perfect: Duration::from_secs_f64(2500.0 / 60_000.0),
                good: Duration::from_secs_f64(0.1),
                hold: Duration::from_secs_f64(0.1),
                miss: Duration::from_secs_f64(10_000.0 / 60_000.0),
            },
            multiplayer: false,
            user_id: "Player".into(),
            practice_setup: false,
        }
    }

    fn reset_canvas(&mut self) {
        let vgfx = self.lua.app_data_mut::<RefMut<Vgfx>>().unwrap();
        let vgfx = vgfx.write().unwrap();
        let canvas = &mut vgfx.canvas.lock().unwrap();
        canvas.flush();
        canvas.reset();
        canvas.reset_transform();
        canvas.reset_scissor();
    }

    fn on_hit(&mut self, hit_rating: HitRating) {
        self.hit_ratings.push(hit_rating);

        let last_score = self.real_score;
        self.real_score += match hit_rating {
            HitRating::Crit { .. } => 2,
            HitRating::Good { .. } => 1,
            _ => 0,
        };

        let combo_updated = match hit_rating {
            HitRating::Crit { .. } | HitRating::Good { .. } => {
                self.combo += 1;
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
            if let Ok(update_score) = self.lua.globals().get::<_, Function>("update_combo") {
                crate::log_result!(update_score.call::<_, ()>(self.combo));
            }
        }

        if last_score != self.real_score {
            if let Ok(update_score) = self.lua.globals().get::<_, Function>("update_score") {
                crate::log_result!(update_score.call::<_, ()>(self.calculate_display_score()));
            }
        }

        let button_hit = self.lua.globals().get::<_, Function>("button_hit");
        let laser_slam_hit = self.lua.globals().get::<_, Function>("laser_slam_hit");

        match hit_rating {
            HitRating::Crit {
                tick,
                delta,
                time: _,
            } => match tick.tick {
                ScoreTick::Chip { lane } => {
                    self.beam_colors_current[lane] = (self.beam_colors[2] / 255.0).into();
                    if let Ok(button_hit) = button_hit {
                        crate::log_result!(button_hit.call::<_, ()>((lane, 2, delta)));
                    }
                }
                ScoreTick::Slam { lane, start, end } => {
                    self.camera.shakes.push(CameraShake::new(
                        ((start - end).abs() * 2.0).to_radians() as _,
                        (end - start).signum() as _,
                        20.0,
                        100.0,
                    ));

                    if let Some(slam_sample) = self.slam_sample.clone() {
                        self.mixer.add(slam_sample.convert_samples()); //TODO: Amplyfy with slam volume
                    }

                    if let Ok(laser_slam_hit) = laser_slam_hit {
                        log_result!(laser_slam_hit.call::<_, ()>((end - start, start, end, lane)));
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
                    if let Ok(near_hit) = self.lua.globals().get::<_, Function>("near_hit") {
                        log_result!(near_hit.call::<_, ()>(delta < 0.0));
                    }
                    if let Ok(button_hit) = button_hit {
                        log_result!(button_hit.call::<_, ()>((lane, 1, delta)));
                    }
                    self.beam_colors_current[lane] = (self.beam_colors[1] / 255.0).into()
                }
            }
            HitRating::Miss {
                tick,
                delta: _,
                time: _,
            } if tick.y > self.current_tick => {
                if let ScoreTick::Chip { lane } = tick.tick {
                    self.beam_colors_current[lane] = (self.beam_colors[0] / 255.0).into();
                    if let Ok(button_hit) = button_hit {
                        log_result!(button_hit.call::<_, ()>((lane, 0, 0)));
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
                        //Button press miss, not idle miss
                        self.beam_colors_current[lane] = (self.beam_colors[3] / 255.0).into();
                    }
                    if let Ok(button_hit) = button_hit {
                        log_result!(button_hit.call::<_, ()>((lane, 0, 0)));
                    }
                }
            }
            _ => {}
        }

        self.gauge.on_hit(hit_rating);
    }

    fn calculate_display_score(&self) -> u64 {
        let max = self.score_summary.total as u64 * 2;

        10_000_000_u64 * self.real_score / max
    }

    fn process_tick(
        &mut self,
        tick: PlacedScoreTick,
        chip_miss_tick: u32,
        slam_miss_tick: u32,
    ) -> HitRating {
        let time = self.current_time().as_secs_f64() * 1000.0;
        match tick.tick {
            ScoreTick::Hold { lane } => {
                if self
                    .input_state
                    .is_button_held((lane as u8).into())
                    .is_some()
                {
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
                if (self.laser_cursors[lane] - pos).abs() < LASER_THRESHOLD {
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
                } else if delta.abs() < (self.lua_game_state.hit_window.good.as_secs_f64() * 1000.0)
                    && contains_cursor
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
            - if let Some(bgm) = &self.chart.audio.bgm {
                bgm.offset as f64
            } else {
                0.0
            }
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

        let playback_ms = self.playback.get_ms();
        let timing_delta = playback_ms.sub(time.as_secs_f64() * 1000.0);
        if playback_ms > 0.0 {
            self.sync_delta.push_front(timing_delta);
            if self.sync_delta.len() > AVG_DELTA_LEN {
                self.sync_delta.pop_back();
            }
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

        if self.current_tick >= self.duration && !self.results_requested {
            self.control_tx
                .as_ref()
                .unwrap()
                .send(ControlMessage::Result {
                    song: self.song.clone(),
                    diff_idx: self.diff_idx,
                    score: self.calculate_display_score() as u32,
                    gauge: self.gauge.value(),
                    hit_ratings: std::mem::take(&mut self.hit_ratings),
                })
                .unwrap();

            self.results_requested = true;
        }
        let missed_chip_tick = self.chart.ms_to_tick(
            self.with_offset(
                time.saturating_sub(self.lua_game_state.hit_window.good)
                    .as_secs_f64()
                    * 1000.0,
            ),
        );

        for (side, ((laser_active, laser_target), wide)) in self
            .laser_active
            .iter_mut()
            .zip(self.laser_target.iter_mut())
            .zip(self.laser_wide.iter_mut())
            .enumerate()
        {
            let was_none = laser_target.is_none();
            *laser_target = self.chart.note.laser[side].value_at(self.current_tick as f64);
            *wide = self.chart.note.laser[side].wide_at(self.current_tick as f64);
            *laser_active = if let Some(val) = laser_target {
                (*val - self.laser_cursors[side]).abs() < LASER_THRESHOLD
            } else {
                false
            };

            if was_none && laser_target.is_some() {
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

        _ = if let Some(f) = laser_freq {
            let freq = 80.0f32 * (8000.0f32 / 80.0f32).powf(f as f32);
            self.biquad_control.send((
                Some(BiQuadState::new(BiQuadType::Peaking(10.0), SQRT_2, freq)),
                Some((1.0 - (f - 0.5).abs() * 1.99).powf(0.1) as f32),
            ))
        } else {
            self.biquad_control.send((None, Some(0.0)))
        };

        self.target_roll = match self.laser_target {
            [Some(l), Some(r)] => Some(r + l - 1.0),
            [Some(l), None] => Some(l),
            [None, Some(r)] => Some(r - 1.0),
            _ => None,
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
                    .map(|x| matches!(x.tick, ScoreTick::Slam { .. }))
            };
            if *assist_ticks > 0 && !next_laser_is_slam().unwrap_or_default() {
                self.laser_cursors[side] = self.chart.note.laser[side]
                    .value_at(self.current_tick as f64)
                    .unwrap_or(self.laser_cursors[side]);
            }
            *assist_ticks = assist_ticks.saturating_sub(1);
        }

        let mut i = 0;
        while i < self.score_ticks.len() {
            if self.score_ticks[i].y > self.current_tick {
                break;
            }

            match self.process_tick(self.score_ticks[i], missed_chip_tick, missed_chip_tick) {
                HitRating::None => i += 1,
                r => {
                    self.on_hit(r);
                    self.score_ticks.remove(i);
                }
            }
        }

        self.playback.set_fx_enable(
            self.input_state
                .is_button_held(UscButton::FX(kson::Side::Left))
                .is_some(),
            self.input_state
                .is_button_held(UscButton::FX(kson::Side::Right))
                .is_some(),
        );

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

        self.gauge = Gauge::Normal {
            chip_gain,
            tick_gain,
            value: 0.0,
        };

        self.control_tx = Some(app_control_tx);
        lua_provider.register_libraries(self.lua.clone(), "gameplay.lua")?;
        Ok(())
    }

    fn debug_ui(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
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

                        if ui
                            .add(Slider::new(&mut self.current_tick, 0..=self.duration))
                            .changed()
                        {
                            let new_time = self.chart.tick_to_ms(self.current_tick);
                            self.playback.set_poistion(new_time);

                            self.zero_time =
                                SystemTime::now().sub(Duration::from_millis(new_time as _))
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
                                .filter(|x| x.y > self.current_tick)
                                .find(|x| match x.tick {
                                    ScoreTick::Chip { lane } | ScoreTick::Hold { lane } => {
                                        lane == i
                                    }
                                    _ => false,
                                })
                                .map(|x| x.y)
                                .unwrap_or(u32::MAX)
                                .saturating_sub(self.current_tick);
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
                            self.chart.note.laser[0].value_at(self.current_tick as f64)
                        {
                            ui.add(egui::Slider::new(&mut lval, 0.0..=1.0));
                        }

                        ui.end_row();

                        ui.label("Right");
                        if let Some(mut rval) =
                            self.chart.note.laser[1].value_at(self.current_tick as f64)
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
                                .direction_at(self.current_tick as f64)
                                .map(|x| x.total_cmp(&0.0))
                        ));
                        ui.end_row();

                        ui.label("Right");
                        ui.label(format!(
                            "{:?}",
                            self.chart.note.laser[1]
                                .direction_at(self.current_tick as f64)
                                .map(|x| x.total_cmp(&0.0))
                        ));
                        ui.end_row();

                        ui.label("Stats");
                        ui.add(
                            egui::Label::new(format!("{:#?}", &self.beam_colors_current))
                                .wrap(false),
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

        self.camera.view_size = vec2(viewport.width as f32, viewport.height as f32);
        if self.intro_done && !self.playback.is_playing() {
            self.zero_time = SystemTime::now() + LEADIN;
            if !self.playback.play() {
                panic!("Could not play")
            };

            let (biquad_control, biquad_events) = std::sync::mpsc::channel();

            self.biquad_control = biquad_control;

            self.mixer.add(owned_source(
                biquad(
                    self.playback.get_source().expect("Audio not loaded"),
                    BiQuadState::new(BiQuadType::AllPass, SQRT_2, 100.0),
                    Some(biquad_events),
                ),
                self.source_owner.0.clone(),
            ));
        }

        let leadin_ms = self.playback.get_ms().min(0.0);

        let time = self.current_time();
        let time_ms = time.as_secs_f64() * 1000.0 + leadin_ms;

        //Update roll
        {
            profile_scope!("Update camera");
            let max_roll_speed = dt / kson::beat_in_ms(self.chart.bpm_at_tick(self.current_tick));
            self.current_roll = if let Some(target_roll) = self.target_roll {
                if self.current_roll - target_roll < 0.0 {
                    (self.current_roll + max_roll_speed * 2.0).min(target_roll)
                } else {
                    (self.current_roll - max_roll_speed * 2.0).max(target_roll)
                }
            } else if self.current_roll.is_sign_negative() {
                (self.current_roll + max_roll_speed).min(0.0)
            } else {
                (self.current_roll - max_roll_speed).max(0.0)
            };

            self.camera.tilt = self.current_roll as f32 * 12.5;

            self.view.cursor = self.with_offset(time.as_secs_f64() * 1000.0) + leadin_ms;

            self.current_tick = self.chart.ms_to_tick(self.view.cursor);
            self.camera.radius = 1.1
                + 0.6
                    * self
                        .chart
                        .camera
                        .cam
                        .body
                        .zoom
                        .value_at(self.current_tick as f64) as f32;
            self.camera.angle = (130.0
                + self
                    .chart
                    .camera
                    .cam
                    .body
                    .rotation_x
                    .value_at(self.current_tick as f64)
                    * 30.0) as f32;

            self.camera.shakes.retain_mut(|x| {
                x.tick(dt as _);
                !x.completed()
            });
        }
        let td_camera: Camera = Camera::from(&self.camera);
        if let Some(bg) = self.background.as_mut() {
            bg.render(
                dt,
                &td_camera,
                time_ms,
                &self.chart,
                self.current_tick,
                self.camera.tilt,
                self.gauge.is_cleared(),
            );
        }

        self.beam_colors_current
            .iter_mut()
            .for_each(|c| c[3] = (c[3] - dt as f32 / 200.0).max(0.0));

        let new_lua_state = self.lua_game_state(viewport, &td_camera);
        if new_lua_state != self.lua_game_state {
            self.lua_game_state = new_lua_state;
            log_result!(self
                .lua
                .globals()
                .set("gameplay", self.lua.to_value(&self.lua_game_state).unwrap()));
        }

        //Set glow/hit states
        let object_glow = ((time_ms as f32 % 100.0) / 50.0 - 1.0).abs() * 0.5 + 0.5;
        let hit_state = ((time_ms / 50.0) % 2.0) as i32 + 2;
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

        //TODO: Set hold glow state

        let buttons_held: std::collections::HashSet<_> = (0..6usize)
            .filter(|x| {
                self.input_state
                    .is_button_held(UscButton::from(*x as u8))
                    .is_some()
            })
            .collect();

        target.render(&td_camera, [&self.track_shader], &[]);
        let render_data = self.view.render(
            &self.chart,
            td_context,
            buttons_held,
            self.beam_colors_current,
        );

        self.fx_long_shaders.draw_instanced_camera(
            &td_camera,
            render_data.fx_hold,
            |material, transform, (hold, active)| {
                material.use_uniform("world", transform * hold);
                let (glow, state) = match active {
                    HoldState::Idle => (0.6, 1),
                    HoldState::Hit => (object_glow, hit_state),
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

        target.render(&td_camera, self.laser_shaders.iter().flatten(), &[]);

        if !self.intro_done {
            if let Ok(func) = self.lua.globals().get::<_, Function>("render_intro") {
                match func.call::<_, bool>(dt / 1000.0) {
                    Err(e) => {
                        log::error!("{}", e);
                    }
                    Ok(intro_complete) => self.intro_done = intro_complete,
                };
            }
        }

        if let Ok(func) = self.lua.globals().get::<_, Function>("render_crit_base") {
            if let Err(e) = func.call::<_, ()>(dt / 1000.0) {
                log::error!("{}", e);
            };
        }
        self.reset_canvas();

        if let Some(fg) = self.foreground.as_mut() {
            fg.render(
                dt,
                &td_camera,
                time_ms,
                &self.chart,
                self.current_tick,
                self.camera.tilt,
                self.gauge.is_cleared(),
            );
        }

        if let Ok(func) = self.lua.globals().get::<_, Function>("render_crit_overlay") {
            if let Err(e) = func.call::<_, ()>(dt / 1000.0) {
                log::error!("{}", e);
            };
        }
        self.reset_canvas();

        if let Ok(func) = self.lua.globals().get::<_, Function>("render") {
            if let Err(e) = func.call::<_, ()>(dt / 1000.0) {
                log::error!("{}", e);
            };
        }
        self.reset_canvas();
        if self.draw_axis_guides {
            let axes = three_d::Axes::new(td_context, 0.01, 0.30);
            target.render(&td_camera, [axes], &[]);
        }
    }

    fn on_event(
        &mut self,
        event: &game_loop::winit::event::Event<crate::button_codes::UscInputEvent>,
    ) {
        if let game_loop::winit::event::Event::UserEvent(UscInputEvent::Laser(ls, timestamp)) =
            event
        {
            //TODO: Slam detection, or always handle slam ticks in ticking function?

            for (side, index) in [(kson::Side::Left, 0), (kson::Side::Right, 1)] {
                let delta = ls.get_axis(side).delta as f64;

                if self.input_state.is_button_held(UscButton::Start).is_some() {
                    self.view.hispeed += delta as f32 * 0.1;
                    self.view.hispeed = self.view.hispeed.clamp(0.1, 10.0);
                }

                let input_dir = delta.total_cmp(&0.0);
                match input_dir {
                    Ordering::Less => self.laser_latest_dir_inputs[index][0] = *timestamp,
                    Ordering::Equal => {}
                    Ordering::Greater => self.laser_latest_dir_inputs[index][1] = *timestamp,
                }

                self.laser_cursors[index] = if self.laser_target[index].is_some() {
                    let new_pos = (self.laser_cursors[index] + delta).clamp(0.0, 1.0);
                    let target_value =
                        self.chart.note.laser[index].value_at(self.current_tick as f64);

                    let target_dir = self.chart.note.laser[index]
                        .direction_at(self.current_tick as f64)
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
        } = self.lua_game_state.hit_window;

        let last_tick = self.chart.ms_to_tick(
            self.with_offset(self.current_time().as_secs_f64() * 1000.0)
                + miss.as_secs_f64() * 1000.0,
        ) + 1;
        let mut hittable_ticks = self.score_ticks.iter().take_while(|x| x.y < last_tick);
        let mut hit_rating = HitRating::None;
        let button_num = Into::<u8>::into(button);

        match button {
            crate::button_codes::UscButton::BT(_) | crate::button_codes::UscButton::FX(_) => {
                if let Some((index, score_tick)) = hittable_ticks.find_position(|x| {
                    if let ScoreTick::Chip { lane } = x.tick {
                        lane == button_num as usize
                    } else {
                        false
                    }
                }) {
                    let tick = *score_tick;
                    let ms = self.chart.tick_to_ms(score_tick.y);
                    let time = self.with_offset(
                        timestamp
                            .duration_since(self.zero_time)
                            .unwrap_or(Duration::ZERO)
                            .as_secs_f64()
                            * 1000.0,
                    );

                    let delta = ms - time;
                    let abs_delta = Duration::from_secs_f64(delta.abs() / 1000.0);
                    log::info!("Hit delta: {}", delta);

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
                        }
                    }
                }
            }
            crate::button_codes::UscButton::Back => self.closed = true,
            _ => {}
        }
        if let HitRating::None = hit_rating {
            if (button_num as usize) < self.beam_colors_current.len() {
                self.beam_colors_current[button_num as usize] =
                    (self.beam_colors[3] / 255.0).into();
            }
        }
    }

    fn name(&self) -> &str {
        "Game"
    }
}

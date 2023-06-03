use crate::{
    button_codes::UscInputEvent,
    input_state::InputState,
    scene::{Scene, SceneData},
    shaded_mesh::ShadedMesh,
    songselect::Song,
    vg_ui::Vgfx,
    ControlMessage,
};
use itertools::Itertools;
use kson::{
    score_ticks::{PlacedScoreTick, ScoreTick, ScoreTickSummary, ScoreTicker},
    Chart, Graph,
};
use puffin::profile_function;
use rodio::Source;
use serde::{Deserialize, Serialize};
use tealr::mlu::mlua::{Function, Lua, LuaSerdeExt};
pub struct Game {
    view: ChartView,
    chart: kson::Chart,
    camera_pos: Vec3,
    time: f64,
    duration: f64,
    fx_long_shaders: [ShadedMesh; 2],
    bt_long_shaders: [ShadedMesh; 2],
    fx_chip_shaders: [ShadedMesh; 2],
    laser_shaders: [[ShadedMesh; 2]; 2],
    track_shader: [ShadedMesh; 1],
    bt_chip_shader: [ShadedMesh; 1],
    camera: three_d::Camera,
    lua_game_state: LuaGameState,
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
    input_state: Arc<InputState>,
    laser_cursors: [f64; 2],
    laser_active: [bool; 2],
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

impl Gauge {
    pub fn on_hit(&mut self, rating: HitRating) {
        match self {
            Gauge::None => {}
            Gauge::Normal {
                chip_gain,
                tick_gain,
                value,
            } => match rating {
                HitRating::Crit(HitType::Chip | HitType::Slam) => *value += *chip_gain,
                HitRating::Crit(HitType::Hold | HitType::Laser) => *value += *tick_gain,
                HitRating::Good(_) => *value += *chip_gain / 3.0, //Only chips can have a "good" rating
                HitRating::Miss(HitType::Chip | HitType::Slam) => *value -= 0.02,
                HitRating::Miss(HitType::Hold | HitType::Laser) => *value -= 0.02 / 4.0,
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
}

#[derive(Debug, Clone, Copy)]
enum HitRating {
    None,
    Crit(HitType),
    Good(HitType),
    Miss(HitType),
}

#[derive(Debug, Clone, Copy)]
enum HitType {
    Laser,
    Hold,
    Chip,
    Slam,
}

impl From<&Gauge> for LuaGauge {
    fn from(value: &Gauge) -> Self {
        match value {
            Gauge::Normal {
                chip_gain,
                tick_gain,
                value,
            } => LuaGauge {
                gauge_type: 0,
                options: 0,
                value: *value,
                name: "Normal".into(),
            },
            Gauge::None => LuaGauge {
                gauge_type: 0,
                options: 0,
                value: 0.0,
                name: "".to_string(),
            },
        }
    }
}

struct TrackRenderMeshes {
    fx_hold: CpuMesh,
    fx_hold_active: CpuMesh,
    bt_hold: CpuMesh,
    bt_hold_active: CpuMesh,
    fx_chip: CpuMesh,
    fx_chip_sample: CpuMesh,
    bt_chip: CpuMesh,
    lasers: [CpuMesh; 4],
}
pub struct GameData {
    song: Arc<Song>,
    diff_idx: usize,
    context: three_d::Context,
    chart: kson::Chart,
    skin_folder: PathBuf,
    audio: Box<dyn Source<Item = f32> + Send>,
}

pub fn extend_mesh(a: CpuMesh, b: CpuMesh) -> CpuMesh {
    let CpuMesh {
        mut positions,
        indices,
        normals,
        tangents,
        uvs,
        colors,
    } = a;

    let index_offset = positions.len();

    let CpuMesh {
        positions: b_positions,
        indices: b_indices,
        normals: _b_normals,
        tangents: _b_tangents,
        uvs: b_uvs,
        colors: _b_colors,
    } = b;

    let indices = match (indices.into_u32(), b_indices.into_u32()) {
        (None, None) => Indices::None,
        (None, Some(mut b)) => {
            b.iter_mut().for_each(|idx| *idx += index_offset as u32);
            Indices::U32(b)
        }
        (Some(a), None) => Indices::U32(a),
        (Some(mut a), Some(mut b)) => {
            b.iter_mut().for_each(|idx| *idx += index_offset as u32);
            a.append(&mut b);
            Indices::U32(a)
        }
    };
    {
        match &mut positions {
            Positions::F32(a) => a.append(&mut b_positions.into_f32()),
            Positions::F64(a) => a.append(&mut b_positions.into_f64()),
        }
    }

    let uvs: Option<Vec<_>> = Some(uvs.iter().chain(b_uvs.iter()).flatten().copied().collect());

    let mut res = CpuMesh {
        positions,
        indices,
        normals,
        tangents,
        uvs,
        colors,
    };

    res.compute_normals();
    res.compute_tangents();

    res
}

impl GameData {
    pub fn new(
        context: three_d::Context,
        song: Arc<Song>,
        diff_idx: usize,
        chart: kson::Chart,
        skin_folder: PathBuf,
        audio: Box<dyn Source<Item = f32> + Send>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            context,
            chart,
            skin_folder,
            diff_idx,
            song,
            audio,
        })
    }
}

impl SceneData for GameData {
    fn make_scene(self: Box<Self>, input_state: Arc<InputState>) -> Box<dyn Scene> {
        let Self {
            context,
            chart,
            skin_folder,
            diff_idx,
            song,
            audio,
        } = *self;
        profile_function!();

        let mesh_transform = Mat4::from_angle_x(Deg(90.0));

        let mut shader_folder = skin_folder.clone();
        let mut texture_folder = skin_folder.clone();
        shader_folder.push("shaders");
        texture_folder.push("textures");
        texture_folder.push("dummy.png");

        let mut fx_long_shader = ShadedMesh::new(&context, "holdbutton", &shader_folder)
            .expect("Failed to load shader:");
        let mut fx_long_shader_active = ShadedMesh::new(&context, "holdbutton", &shader_folder)
            .expect("Failed to load shader:");

        fx_long_shader.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("fxbuttonhold.png"),
            (false, false),
        );
        fx_long_shader_active.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("fxbuttonhold.png"),
            (false, false),
        );

        let mut bt_long_shader = ShadedMesh::new(&context, "holdbutton", &shader_folder)
            .expect("Failed to load shader:");
        let mut bt_long_shader_active = ShadedMesh::new(&context, "holdbutton", &shader_folder)
            .expect("Failed to load shader:");

        bt_long_shader.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("buttonhold.png"),
            (false, false),
        );
        bt_long_shader_active.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("buttonhold.png"),
            (false, false),
        );

        let mut fx_chip_shader =
            ShadedMesh::new(&context, "button", &shader_folder).expect("Failed to load shader:");
        let mut fx_chip_shader_sample =
            ShadedMesh::new(&context, "button", &shader_folder).expect("Failed to load shader:");

        fx_chip_shader.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("fxbutton.png"),
            (false, false),
        );
        fx_chip_shader_sample.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("fxbutton.png"),
            (false, false),
        );

        let mut bt_chip_shader =
            ShadedMesh::new(&context, "button", &shader_folder).expect("Failed to load shader:");

        bt_chip_shader.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("button.png"),
            (false, false),
        );

        let mut track_shader =
            ShadedMesh::new(&context, "track", &shader_folder).expect("Failed to load shader:");
        track_shader.set_data_mesh(
            &context,
            &xy_rect(Vec3::zero(), vec2(1.0, ChartView::TRACK_LENGTH * 2.0)),
        );

        track_shader.set_param("lCol", Color::BLUE.to_vec4());
        track_shader.set_param("rCol", Color::RED.to_vec4());

        track_shader.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("track.png"),
            (false, false),
        );

        let mut laser_left =
            ShadedMesh::new(&context, "laser", &shader_folder).expect("Failed to load shader:");
        let mut laser_left_active =
            ShadedMesh::new(&context, "laser", &shader_folder).expect("Failed to load shader:");

        let mut laser_right =
            ShadedMesh::new(&context, "laser", &shader_folder).expect("Failed to load shader:");
        let mut laser_right_active =
            ShadedMesh::new(&context, "laser", &shader_folder).expect("Failed to load shader:");

        laser_left.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("laser_l.png"),
            (false, true),
        );
        laser_left_active.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("laser_l.png"),
            (false, true),
        );
        laser_right.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("laser_r.png"),
            (false, true),
        );
        laser_right_active.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("laser_r.png"),
            (false, true),
        );

        laser_left.set_blend(Blend::ADD);
        laser_left_active.set_blend(Blend::ADD);
        laser_right.set_blend(Blend::ADD);
        laser_right_active.set_blend(Blend::ADD);

        let mut playback =
            kson_music_playback::AudioPlayback::try_new().expect("Failed to open playback channel");

        playback.open(audio, "Game").expect("Failed to load audio");
        playback.build_effects(&chart);

        Box::new(
            Game::new(
                chart,
                &skin_folder,
                &context,
                [
                    fx_long_shader.with_transform(mesh_transform),
                    fx_long_shader_active.with_transform(mesh_transform),
                ],
                [
                    bt_long_shader.with_transform(mesh_transform),
                    bt_long_shader_active.with_transform(mesh_transform),
                ],
                [
                    fx_chip_shader.with_transform(mesh_transform),
                    fx_chip_shader_sample.with_transform(mesh_transform),
                ],
                [
                    [
                        laser_left.with_transform(mesh_transform),
                        laser_left_active.with_transform(mesh_transform),
                    ],
                    [
                        laser_right.with_transform(mesh_transform),
                        laser_right_active.with_transform(mesh_transform),
                    ],
                ],
                [track_shader.with_transform(mesh_transform)],
                [bt_chip_shader.with_transform(mesh_transform)],
                song,
                diff_idx,
                playback,
                input_state,
            )
            .unwrap(),
        )
    }
}

fn camera_to_screen(camera: &Camera, point: Vec3, screen: Vec2) -> Vec2 {
    let Vector3 { x, y, z } = point;
    let cameraSpace = camera.view().transform_point(three_d::Point3 { x, y, z });
    let mut screenSpace = camera.projection().transform_point(cameraSpace);
    screenSpace.y = -screenSpace.y;
    screenSpace *= 0.5f32;
    screenSpace += vec3(0.5, 0.5, 0.5);
    vec2(screenSpace.x * screen.x, screenSpace.y * screen.y)
}

impl Game {
    pub fn new(
        chart: Chart,
        skin_root: &PathBuf,
        td: &three_d::Context,
        fx_long_shaders: [ShadedMesh; 2],
        bt_long_shaders: [ShadedMesh; 2],
        fx_chip_shaders: [ShadedMesh; 2],
        laser_shaders: [[ShadedMesh; 2]; 2],
        track_shader: [ShadedMesh; 1],
        bt_chip_shader: [ShadedMesh; 1],
        song: Arc<Song>,
        diff_idx: usize,
        playback: kson_music_playback::AudioPlayback,
        input_state: Arc<InputState>,
    ) -> Result<Self> {
        let mut view = ChartView::new(skin_root, td);
        view.build_laser_meshes(&chart);
        let duration = chart.get_last_tick();
        let duration = chart.tick_to_ms(duration);

        let score_ticks = kson::score_ticks::generate_score_ticks(&chart);

        let mut res = Self {
            song,
            diff_idx,
            intro_done: false,
            lua: Rc::new(Lua::new()),
            chart,
            view,
            duration,
            time: 0f64,
            camera_pos: vec3(0.0, 1.0, 1.0),
            bt_chip_shader,
            track_shader,
            bt_long_shaders,
            fx_chip_shaders,
            fx_long_shaders,
            laser_shaders,
            camera: Camera::new_orthographic(
                Viewport {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                Vec3::zero(),
                Vec3::unit_x(),
                Vec3::unit_z(),
                1.0,
                1.0,
                10.0,
            ),
            lua_game_state: LuaGameState::default(),
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
        };
        res.set_track_uniforms();
        Ok(res)
    }

    fn set_track_uniforms(&mut self) {
        self.track_shader
            .iter_mut()
            .chain(self.fx_long_shaders.iter_mut())
            .chain(self.bt_long_shaders.iter_mut())
            .chain(self.fx_chip_shaders.iter_mut())
            .chain(self.bt_chip_shader.iter_mut())
            .chain(self.laser_shaders.iter_mut().flatten())
            .for_each(|shader| {
                shader.set_param("trackPos", 0.0);
                shader.set_param("trackScale", 1.0);
                shader.set_param("hiddenCutoff", 0.0);
                shader.set_param("hiddenFadeWindow", 100.0);
                shader.set_param("suddenCutoff", 10.0);
                shader.set_param("suddenFadeWindow", 1000.0);
            });

        self.laser_shaders
            .iter_mut()
            .flatten()
            .for_each(|laser| laser.set_param("objectGlow", 1.0));
        self.laser_shaders[0]
            .iter_mut()
            .for_each(|ll| ll.set_param("color", Color::BLUE.to_vec4()));
        self.laser_shaders[1]
            .iter_mut()
            .for_each(|rl| rl.set_param("color", Color::RED.to_vec4()));
    }

    fn lua_game_state(&self, viewport: Viewport) -> LuaGameState {
        let screen = vec2(viewport.width as f32, viewport.height as f32);
        let track_center = camera_to_screen(&self.camera, Vec3::zero(), screen);

        let track_left = camera_to_screen(&self.camera, Vec3::unit_x() * -1.0, screen);
        let track_right = camera_to_screen(&self.camera, Vec3::unit_x(), screen);
        let crit_line = track_right - track_left;
        let rotation = crit_line.y.atan2(crit_line.x);

        LuaGameState {
            title: self.chart.meta.title.clone(),
            artist: self.chart.meta.artist.clone(),
            jacket_path: self.song.as_ref().difficulties[self.diff_idx]
                .jacket_path
                .clone(),
            demo_mode: false,
            difficulty: self.chart.meta.difficulty,
            level: self.chart.meta.level,
            progress: self.time as f32 / self.duration as f32,
            hispeed: self.view.hispeed,
            hispeed_adjust: 0,
            bpm: self
                .chart
                .bpm_at_tick(self.chart.ms_to_tick(self.time as f64)) as f32,
            gauge: LuaGauge::from(&self.gauge),
            hidden_cutoff: 0.0,
            sudden_cutoff: 0.0,
            hidden_fade: 0.0,
            sudden_fade: 0.0,
            autoplay: false,
            combo_state: 0,
            note_held: [false; 6],
            laser_active: [false; 2],
            score_replays: Vec::new(),
            crit_line: CritLine {
                x: track_center.x as i32,
                y: track_center.y as i32,
                x_offset: 0.0,
                rotation,
                cursors: [
                    Cursor::new(
                        self.laser_cursors[0] as f32,
                        &self.camera,
                        if self.laser_active[0] { 1.0 } else { 0.0 },
                    ),
                    Cursor::new(
                        self.laser_cursors[1] as f32,
                        &self.camera,
                        if self.laser_active[1] { 1.0 } else { 0.0 },
                    ),
                ],
                line: Line {
                    x1: track_left.x,
                    y1: track_left.y,
                    x2: track_right.x,
                    y2: track_right.y,
                },
            },
            hit_window: HitWindow {
                variant: 1,
                perfect: 2_500.0 / 60.0,
                good: 6_000.0 / 60.0,
                hold: 6_000.0 / 60.0,
                miss: 10_000.0 / 60.0,
            },
            multiplayer: false,
            user_id: "Player".into(),
            practice_setup: false,
        }
    }

    fn reset_canvas(&mut self) {
        let vgfx = self.lua.app_data_mut::<Arc<Mutex<Vgfx>>>().unwrap();
        let vgfx = vgfx.lock().unwrap();
        let canvas = &mut vgfx.canvas.lock().unwrap();
        canvas.flush();
        canvas.reset();
        canvas.reset_transform();
        canvas.reset_scissor();
    }

    fn on_hit(&mut self, hit_rating: HitRating) {
        let last_score = self.real_score;
        self.real_score += match hit_rating {
            HitRating::Crit(_) => 2,
            HitRating::Good(_) => 1,
            _ => 0,
        };

        let combo_updated = match hit_rating {
            HitRating::Crit(_) | HitRating::Good(_) => {
                self.combo += 1;
                true
            }
            HitRating::Miss(_) => {
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
                update_score.call::<_, ()>(self.combo);
            }
        }

        if last_score != self.real_score {
            if let Ok(update_score) = self.lua.globals().get::<_, Function>("update_score") {
                update_score.call::<_, ()>(self.calculate_display_score());
            }
        }

        self.gauge.on_hit(hit_rating);
    }

    fn calculate_display_score(&self) -> u64 {
        let max = self.score_summary.total as u64 * 2;

        10_000_000_u64 * self.real_score / max
    }

    fn process_tick(&self, tick: &PlacedScoreTick, chip_miss_tick: u32) -> HitRating {
        match tick.tick {
            ScoreTick::Hold { lane } => {
                if self.input_state.is_button_held((lane as u8).into()) {
                    HitRating::Crit(HitType::Hold)
                } else {
                    HitRating::Miss(HitType::Hold)
                }
            }
            ScoreTick::Laser { lane, pos } => {
                const LASER_WIDTH: f64 = 1.0 / 6.0;
                if (self.laser_cursors[lane] - pos).abs() < LASER_WIDTH {
                    HitRating::Crit(HitType::Laser)
                } else {
                    HitRating::Miss(HitType::Laser)
                }
            }
            ScoreTick::Slam { lane, start, end } => HitRating::Miss(HitType::Slam),
            ScoreTick::Chip { lane: _ } => {
                if tick.y < chip_miss_tick {
                    HitRating::Miss(HitType::Chip)
                } else {
                    HitRating::None
                }
            }
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
        if self.time >= self.duration && !self.results_requested {
            self.control_tx
                .as_ref()
                .unwrap()
                .send(ControlMessage::Result {
                    song: self.song.clone(),
                    diff_idx: self.diff_idx,
                    score: self.calculate_display_score() as u32,
                    gauge: 0.5,
                });

            self.results_requested = true;
        }
        let missed_chip_tick = self
            .chart
            .ms_to_tick(self.time - self.lua_game_state.hit_window.good);

        let mut i = 0;
        while i < self.score_ticks.len() {
            if self.score_ticks[i].y > self.current_tick {
                break;
            }

            match self.process_tick(&self.score_ticks[i], missed_chip_tick) {
                HitRating::None => i += 1,
                r => {
                    self.on_hit(r);
                    self.score_ticks.remove(i);
                }
            }
        }

        for (side, laser_active) in self.laser_active.iter_mut().enumerate() {
            *laser_active = self.chart.note.laser[side]
                .value_at(self.current_tick as f64)
                .is_some();
            //TODO: Also check ahead
        }

        Ok(())
    }

    fn suspend(&mut self) {
        self.closed = true;
    }

    fn init(
        &mut self,
        load_lua: Rc<dyn Fn(Rc<Lua>, &'static str) -> Result<generational_arena::Index>>,
        app_control_tx: std::sync::mpsc::Sender<crate::ControlMessage>,
    ) -> Result<()> {
        profile_function!();

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
        load_lua(self.lua.clone(), "gameplay.lua")?;
        Ok(())
    }

    fn debug_ui(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        use egui::*;
        Window::new("Camera").show(ctx, |ui| {
            let Vector3 {
                mut x,
                mut y,
                mut z,
            } = self.camera_pos;
            ui.add(Slider::new(&mut x, -10.0..=10.0).logarithmic(true));
            ui.add(Slider::new(&mut y, -10.0..=10.0).logarithmic(true));
            ui.add(Slider::new(&mut z, -10.0..=10.0).logarithmic(true));

            self.camera_pos = vec3(x, y, z);
        });
        Window::new("Game Data").show(ctx, |ui| {
            egui::Grid::new("gameplay_data")
                .num_columns(2)
                .spacing([40.0, 4.0])
                .show(ui, |ui| {
                    let current_tick = self.chart.ms_to_tick(self.time);

                    ui.label("Time");

                    if ui
                        .add(Slider::new(&mut self.time, 0.0..=self.duration))
                        .changed()
                    {
                        self.playback.set_poistion(self.time);
                    }

                    ui.end_row();

                    ui.label("HiSpeed");
                    ui.add(Slider::new(&mut self.view.hispeed, 0.001..=2.0));

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
                                ScoreTick::Chip { lane } | ScoreTick::Hold { lane } => lane == i,
                                _ => false,
                            })
                            .map(|x| self.chart.tick_to_ms(x.y))
                            .unwrap_or(f64::INFINITY)
                            - self.time;
                        ui.label(match i {
                            0 => "BT A",
                            1 => "BT B",
                            2 => "BT C",
                            3 => "BT D",
                            4 => "FX L",
                            5 => "FX R",
                            _ => unreachable!(),
                        });
                        ui.add(Slider::new(&mut next_tick, 0.0..=10000.0).logarithmic(true));
                        ui.end_row();
                    }
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
        self.camera = Camera::new_perspective(
            viewport,
            self.camera_pos,
            self.camera_pos + vec3(0.0, -1.0, -4.0),
            Vec3::unit_y(),
            Rad(90.0_f32.to_radians()),
            0.01,
            10000.0,
        );
        if self.intro_done && !self.playback.is_playing() {
            self.playback.play();
        }
        self.view.cursor = self.playback.get_ms();
        self.time = self.view.cursor
            - self
                .chart
                .audio
                .bgm
                .as_ref()
                .map(|x| x.offset as f64)
                .unwrap_or(0.0);
        self.current_tick = self.chart.ms_to_tick(self.time);

        let new_lua_state = self.lua_game_state(viewport);
        if new_lua_state != self.lua_game_state {
            self.lua_game_state = new_lua_state;
            self.lua
                .globals()
                .set("gameplay", self.lua.to_value(&self.lua_game_state).unwrap());
        }

        let render_data = self.view.render(&self.chart, td_context);

        self.bt_chip_shader[0].set_data_mesh(td_context, &render_data.bt_chip);
        self.bt_long_shaders[0].set_data_mesh(td_context, &render_data.bt_hold);
        self.bt_long_shaders[1].set_data_mesh(td_context, &render_data.bt_hold_active);

        self.fx_chip_shaders[0].set_data_mesh(td_context, &render_data.fx_chip);
        self.fx_chip_shaders[1].set_data_mesh(td_context, &render_data.fx_chip_sample);
        self.fx_long_shaders[0].set_data_mesh(td_context, &render_data.fx_hold);
        self.fx_long_shaders[1].set_data_mesh(td_context, &render_data.fx_hold_active);

        self.laser_shaders[0][0].set_data_mesh(td_context, &render_data.lasers[0]);
        self.laser_shaders[0][1].set_data_mesh(td_context, &render_data.lasers[1]);
        self.laser_shaders[1][0].set_data_mesh(td_context, &render_data.lasers[2]);
        self.laser_shaders[1][1].set_data_mesh(td_context, &render_data.lasers[3]);

        target.render(
            &self.camera,
            self.track_shader
                .iter()
                .chain(self.fx_long_shaders.iter())
                .chain(self.bt_long_shaders.iter())
                .chain(self.fx_chip_shaders.iter())
                .chain(self.bt_chip_shader.iter())
                .chain(self.laser_shaders.iter().flatten()),
            &[],
        );

        if !self.intro_done {
            if let Ok(func) = self.lua.globals().get::<_, Function>("render_intro") {
                match func.call::<_, bool>(dt / 1000.0) {
                    Err(e) => {
                        log::error!("{:?}", e.to_string());
                    }
                    Ok(intro_complete) => self.intro_done = intro_complete,
                };
            }
        }

        if let Ok(func) = self.lua.globals().get::<_, Function>("render_crit_base") {
            if let Err(e) = func.call::<_, ()>(dt / 1000.0) {
                log::error!("{:?}", e.to_string());
            };
        }
        self.reset_canvas();

        if let Ok(func) = self.lua.globals().get::<_, Function>("render_crit_overlay") {
            if let Err(e) = func.call::<_, ()>(dt / 1000.0) {
                log::error!("{:?}", e.to_string());
            };
        }
        self.reset_canvas();

        if let Ok(func) = self.lua.globals().get::<_, Function>("render") {
            if let Err(e) = func.call::<_, ()>(dt / 1000.0) {
                log::error!("{:?}", e.to_string());
            };
        }
        self.reset_canvas();

        let axes = three_d::Axes::new(td_context, 0.01, 0.30);
        target.render(&self.camera, [axes], &[]);
    }

    fn on_event(
        &mut self,
        event: &game_loop::winit::event::Event<crate::button_codes::UscInputEvent>,
    ) {
        if let game_loop::winit::event::Event::UserEvent(UscInputEvent::Laser(ls)) = event {
            self.laser_cursors[0] = if self.laser_active[0] {
                (self.laser_cursors[0] + ls.get_axis(kson::Side::Left).delta as f64).clamp(0.0, 1.0)
            } else {
                0.0
            };

            self.laser_cursors[1] = if self.laser_active[1] {
                (self.laser_cursors[1] + ls.get_axis(kson::Side::Right).delta as f64)
                    .clamp(0.0, 1.0)
            } else {
                1.0
            };
        }
    }

    fn on_button_pressed(&mut self, button: crate::button_codes::UscButton) {
        let HitWindow {
            variant: _,
            perfect,
            good,
            hold: _,
            miss,
        } = self.lua_game_state.hit_window;

        //TODO: chart offset shenanigans
        let last_tick = self.chart.ms_to_tick(self.time + miss) + 1;
        let mut hittable_ticks = self.score_ticks.iter().take_while(|x| x.y < last_tick);
        match button {
            crate::button_codes::UscButton::BT(input_lane) => {
                if let Some((index, score_tick)) = hittable_ticks.find_position(|x| {
                    if let ScoreTick::Chip { lane } = x.tick {
                        lane == input_lane as usize
                    } else {
                        false
                    }
                }) {
                    let ms = self.chart.tick_to_ms(score_tick.y);
                    let abs_delta = (ms - self.time).abs();
                    log::info!("Hit delta: {}", ms - self.time);

                    let hit_rating = if abs_delta <= perfect {
                        HitRating::Crit(HitType::Chip)
                    } else if abs_delta <= good {
                        HitRating::Good(HitType::Chip)
                    } else if abs_delta <= miss {
                        HitRating::Miss(HitType::Chip)
                    } else {
                        HitRating::None
                    };

                    self.on_hit(hit_rating);

                    match hit_rating {
                        HitRating::None => {}
                        _ => {
                            self.score_ticks.remove(index);
                        }
                    }
                }
            }
            crate::button_codes::UscButton::FX(side) => {}
            crate::button_codes::UscButton::Back => self.closed = true,
            _ => {}
        }
    }

    fn name(&self) -> &str {
        "Game"
    }
}

use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::{mpsc::Sender, Arc, Mutex},
};

pub struct ChartView {
    pub hispeed: f32,
    pub cursor: f64,
    laser_meshes: [Vec<Vec<GlVertex>>; 2],
    track: CpuMesh,
    pub state: i32,
}

use anyhow::{ensure, Result};
use three_d::{
    vec2, vec3, Blend, Camera, Color, ColorMaterial, CpuMesh, Deg, DepthTest, Indices, InnerSpace,
    Mat3, Mat4, Matrix4, Positions, Rad, RenderStates, Texture2D, Transform, Vec2, Vec3, Vec4,
    Vector3, Viewport, Zero,
};

#[derive(Debug)]
#[repr(C)]
struct GlVec3 {
    x: f32,
    y: f32,
    z: f32,
}

#[derive(Debug)]
#[repr(C)]
struct GlVec2 {
    x: f32,
    y: f32,
}
#[derive(Debug)]
#[repr(C)]
struct GlVertex {
    pos: GlVec3,
    uv: GlVec2,
}

impl GlVertex {
    pub fn new(pos: [f32; 3], uv: [f32; 2]) -> Self {
        GlVertex {
            pos: GlVec3 {
                x: pos[0],
                y: pos[1],
                z: pos[2],
            },
            uv: GlVec2 { x: uv[0], y: uv[1] },
        }
    }
}

fn generate_slam_verts(
    vertices: &mut Vec<GlVertex>,
    start: f32,
    end: f32,
    height: f32,
    xoff: f32,
    y: f32,
    w: f32,
    entry: bool,
    exit: bool,
) {
    let x0 = start.min(end) - xoff;
    let x1 = start.max(end) - xoff - w;
    let y0 = y + height;
    let y1 = y;

    vertices.append(&mut vec![
        GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
        GlVertex::new([y0, 0.0, x1], [0.0, 1.0]),
        GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
        GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
        GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
        GlVertex::new([y1, 0.0, x0], [1.0, 0.0]),
    ]);

    //corners
    {
        /*
        a:
        _____
        |\  |
        | \ |
        |__\|

        b:
        _____
        |  /|
        | / |
        |/__|
        */
        //left
        {
            let x1 = x0;
            let x0 = x0 - w;
            if start > end {
                //b <<<<<
                vertices.append(&mut vec![
                    GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
                    GlVertex::new([y0, 0.0, x1], [1.0, 1.0]),
                    GlVertex::new([y1, 0.0, x0], [0.0, 1.0]),
                    GlVertex::new([y0, 0.0, x1], [0.0, 0.0]),
                    GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
                    GlVertex::new([y1, 0.0, x0], [1.0, 0.0]),
                ]);
            } else {
                //a >>>>>
                vertices.append(&mut vec![
                    GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
                    GlVertex::new([y0, 0.0, x1], [0.0, 1.0]),
                    GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
                    GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
                    GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
                    GlVertex::new([y1, 0.0, x0], [0.0, 0.0]),
                ]);
            }
        }
        //right
        {
            let x0 = x1;
            let x1 = x1 + w;
            if start > end {
                //b <<<<<
                vertices.append(&mut vec![
                    GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
                    GlVertex::new([y0, 0.0, x1], [0.0, 1.0]),
                    GlVertex::new([y1, 0.0, x0], [1.0, 1.0]),
                    GlVertex::new([y0, 0.0, x1], [1.0, 0.0]),
                    GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
                    GlVertex::new([y1, 0.0, x0], [0.0, 0.0]),
                ]);
            } else {
                //a >>>>>
                vertices.append(&mut vec![
                    GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
                    GlVertex::new([y0, 0.0, x1], [1.0, 1.0]),
                    GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
                    GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
                    GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
                    GlVertex::new([y1, 0.0, x0], [1.0, 0.0]),
                ]);
            }
        }
    }

    if entry {
        //entry square
        let x0 = start - w - xoff;
        let x1 = start - xoff;
        let y0 = y;
        let y1 = y - height;

        vertices.append(&mut vec![
            GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
            GlVertex::new([y0, 0.0, x1], [1.0, 0.0]),
            GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
            GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
            GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
            GlVertex::new([y1, 0.0, x0], [0.0, 1.0]),
        ]);
    }
    if exit {
        //exit square
        let x0 = end - w - xoff;
        let x1 = end - xoff;
        let y0 = y + height * 2.0;
        let y1 = y + height;
        vertices.append(&mut vec![
            GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
            GlVertex::new([y0, 0.0, x1], [1.0, 0.0]),
            GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
            GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
            GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
            GlVertex::new([y1, 0.0, x0], [0.0, 1.0]),
        ]);
    }
}

pub fn xz_rect(center: Vec3, size: Vec2) -> CpuMesh {
    let indices = vec![0u8, 1, 2, 2, 3, 0];
    let halfsize_x = size.x / 2.0;
    let halfsize_z = size.y / 2.0;
    let positions = vec![
        center + Vec3::new(-halfsize_x, 0.0, -halfsize_z),
        center + Vec3::new(halfsize_x, 0.0, -halfsize_z),
        center + Vec3::new(halfsize_x, 0.0, halfsize_z),
        center + Vec3::new(-halfsize_x, 0.0, halfsize_z),
    ];
    let normals = vec![
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, 1.0),
    ];
    let tangents = vec![
        Vec4::new(1.0, 0.0, 0.0, 1.0),
        Vec4::new(1.0, 0.0, 0.0, 1.0),
        Vec4::new(1.0, 0.0, 0.0, 1.0),
        Vec4::new(1.0, 0.0, 0.0, 1.0),
    ];
    let uvs = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
    ];
    CpuMesh {
        indices: Indices::U8(indices),
        positions: Positions::F32(positions),
        normals: Some(normals),
        tangents: Some(tangents),
        uvs: Some(uvs),
        ..Default::default()
    }
}

pub fn xy_rect(center: Vec3, size: Vec2) -> CpuMesh {
    let indices = vec![0u8, 1, 2, 2, 3, 0];
    let halfsize_x = size.x / 2.0;
    let halfsize_z = size.y / 2.0;
    let positions = vec![
        center + Vec3::new(-halfsize_x, -halfsize_z, 0.0),
        center + Vec3::new(halfsize_x, -halfsize_z, 0.0),
        center + Vec3::new(halfsize_x, halfsize_z, 0.0),
        center + Vec3::new(-halfsize_x, halfsize_z, 0.0),
    ];

    let uvs = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
    ];
    CpuMesh {
        indices: Indices::U8(indices),
        positions: Positions::F32(positions),
        uvs: Some(uvs),
        ..Default::default()
    }
}

fn plane_normal(a: Vec3, b: Vec3, c: Vec3) -> Vector3<f32> {
    // Calculate the edge vectors formed by the three points
    let ab = b - a;
    let ac = c - a;

    // Use the cross product to get the normal to the plane
    ab.cross(ac).normalize()
}

fn plane_angle(v1: Vector3<f32>, v2: Vector3<f32>, normal: Vector3<f32>) -> f32 {
    // Project the vectors onto the plane
    let v1_on_plane = v1 - (v1.dot(normal) / normal.dot(normal)) * normal;
    let v2_on_plane = v2 - (v2.dot(normal) / normal.dot(normal)) * normal;

    // Calculate the angle between the vectors on the plane
    let dot = v1_on_plane.dot(v2_on_plane);
    let mag = v1_on_plane.magnitude() * v2_on_plane.magnitude();
    (dot / mag).acos()
}

fn draw_line_3d(a: Vec3, b: Vec3, r: f32) -> CpuMesh {
    let mut mesh = CpuMesh::cylinder(8);

    let line_vector = b - a;
    let line_length = line_vector.magnitude();
    let line_direction = line_vector.normalize();

    let rotation_axis = plane_normal(line_direction, Vec3::unit_x(), Vec3::zero());

    //vector difference should make up a plane and rotating along the normal should work?

    let trans = Matrix4::from_translation(a)
        * Matrix4::from_axis_angle(
            rotation_axis,
            Rad(plane_angle(line_direction, Vec3::unit_x(), rotation_axis)),
        )
        * Matrix4::from_nonuniform_scale(line_length, r, r);
    mesh.transform(&trans);

    mesh
}

fn draw_plane(center: Vec3, size: Vec2, normal: Vec3) -> CpuMesh {
    let mut square = CpuMesh::square();
    let plane_matrix = [
        [size.x, 0.0, 0.0, 0.0],
        [0.0, size.y, 0.0, 0.0],
        [normal.x, normal.y, normal.z, 0.0],
        [center.x, center.y, center.z, 1.0],
    ];

    square.transform(&Matrix4::from_cols(
        plane_matrix[0].into(),
        plane_matrix[1].into(),
        plane_matrix[2].into(),
        plane_matrix[3].into(),
    ));
    square
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let h = h % 1.0; // wrap hue value around 1.0
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;

    let (r, g, b) = match h {
        h if h < 0.166_666_67 => (c, x, 0.0),
        h if h < 0.333_333_34 => (x, c, 0.0),
        h if h < 0.5 => (0.0, c, x),
        h if h < 0.666_666_7 => (0.0, x, c),
        h if h < 0.833_333_3 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    [r + m, g + m, b + m]
}

impl ChartView {
    pub const TRACK_LENGTH: f32 = 12.0;

    pub fn new(skin_root: impl AsRef<Path>, td: &three_d::Context) -> Self {
        let _indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
        let mut texure_path = skin_root.as_ref().to_path_buf();
        texure_path.push("textures");
        texure_path.push("file.png");
        td.set_depth_test(three_d::DepthTest::Never);

        let mut textures = three_d_asset::io::load(&[
            texure_path.with_file_name("laser_l.png"),
            texure_path.with_file_name("laser_r.png"),
            texure_path.with_file_name("track.png"),
            texure_path.with_file_name("fxbutton.png"),
            texure_path.with_file_name("button.png"),
        ])
        .unwrap();

        let _laser_texture = Some(Arc::new(Texture2D::new(
            td,
            &textures.deserialize("laser_l").unwrap(),
        )));
        let _laser_render_states = RenderStates {
            blend: Blend::ADD,
            depth_test: DepthTest::Always,
            ..Default::default()
        };

        let track_texture = Arc::new(Texture2D::new(td, &textures.deserialize("track").unwrap()));

        let _track_mat = Rc::new(ColorMaterial {
            color: Color::WHITE,
            texture: Some(three_d::Texture2DRef {
                texture: track_texture,
                transformation: Mat3::from_scale(1.0),
            }),
            render_states: RenderStates {
                depth_test: three_d::DepthTest::Always,
                ..Default::default()
            },
            ..Default::default()
        });

        let track = xy_rect(vec3(0.0, 0.0, 0.0), vec2(1.0, Self::TRACK_LENGTH * 2.0));
        let _button_render_states = RenderStates {
            depth_test: DepthTest::Always,
            ..Default::default()
        };

        ChartView {
            cursor: 0.0,
            hispeed: 1.0,
            laser_meshes: [Vec::new(), Vec::new()],
            track,
            state: 0,
        }
    }

    pub fn build_laser_meshes(&mut self, chart: &kson::Chart) {
        for i in 0..2 {
            self.laser_meshes[i].clear();
            for section in &chart.note.laser[i] {
                let mut section_verts = Vec::new();
                let w = 1.0 / 6.0;
                let (xoff, track_w) = if section.wide() < 2 {
                    (2.0 / 6.0, 5.0 / 6.0)
                } else {
                    (2.0 / 6.0, 11.0 / 12.0)
                };
                let mut is_first = true;
                for se in section.segments() {
                    let s = se[0];
                    let e = se[1];
                    let mut syoff = 0.0_f32;
                    let mut start_value = s.v as f32 * track_w;

                    if let Some(value) = s.vf {
                        let value = value as f32 * track_w;
                        syoff = chart.beat.resolution as f32 / 8.0;
                        generate_slam_verts(
                            &mut section_verts,
                            start_value,
                            value,
                            syoff,
                            xoff,
                            s.ry as f32,
                            w,
                            is_first,
                            false,
                        );
                        start_value = value;
                    }
                    let end_value = e.v as f32 * track_w;
                    let x00 = end_value - w - xoff;
                    let x01 = end_value - xoff;
                    let x10 = start_value - w - xoff;
                    let x11 = start_value - xoff;
                    let y0 = e.ry as f32;
                    let y1 = s.ry as f32 + syoff;

                    section_verts.append(&mut vec![
                        GlVertex::new([y0, 0.0, x00], [0.0, 0.0]),
                        GlVertex::new([y0, 0.0, x01], [1.0, 0.0]),
                        GlVertex::new([y1, 0.0, x11], [1.0, 1.0]),
                        GlVertex::new([y0, 0.0, x00], [0.0, 0.0]),
                        GlVertex::new([y1, 0.0, x10], [0.0, 1.0]),
                        GlVertex::new([y1, 0.0, x11], [1.0, 1.0]),
                    ]);
                    is_first = false;
                }
                if let Some(e) = section.last() {
                    if let Some(value) = e.vf {
                        let start_value = e.v as f32 * track_w;
                        let value = value as f32 * track_w;
                        let syoff = chart.beat.resolution as f32 / 8.0;
                        generate_slam_verts(
                            &mut section_verts,
                            start_value,
                            value,
                            syoff,
                            xoff,
                            e.ry as f32,
                            w,
                            is_first,
                            true,
                        );
                    }
                }
                self.laser_meshes[i].push(section_verts);
            }
        }
    }

    fn render(&mut self, chart: &kson::Chart, td: &three_d::Context) -> TrackRenderMeshes {
        use three_d::prelude::*;
        let view_time = self.cursor - chart.audio.clone().bgm.unwrap().offset as f64;
        let view_offset = if view_time < 0.0 {
            chart.ms_to_tick(view_time.abs() as f64) as i64 //will be weird with early bpm changes
        } else {
            0
        };

        td.set_depth_test(three_d::DepthTest::Never);

        let _glow_state = if (0.0_f32 * 8.0).fract() > 0.5 { 2 } else { 3 };
        let view_tick = chart.ms_to_tick(view_time as f64) as i64 - view_offset;
        let view_distance = (chart.beat.resolution as f32 * 4.0) / self.hispeed;
        let last_view_tick = view_distance.ceil() as i64 + view_tick;
        let first_view_tick = view_tick - view_distance as i64;
        let y_view_div = ((chart.beat.resolution as f32 * 4.0) / self.hispeed) / Self::TRACK_LENGTH;
        let _white_mat = Rc::new(ColorMaterial {
            color: Color::WHITE,
            ..Default::default()
        });

        #[derive(Debug, PartialEq, Eq, Clone, Copy)]
        enum NoteType {
            BtChip,
            BtHold,
            BtHoldActive,
            FxChip,
            FxChipSample,
            FxHold,
            FxHoldActive,
        }
        let mut notes = Vec::new();
        let chip_h = 0.05;

        let _track = self.track.clone();

        for i in 0..4 {
            for n in &chart.note.bt[i] {
                if (n.y as i64) > last_view_tick {
                    break;
                } else if ((n.y + n.l) as i64) < first_view_tick {
                    continue;
                }

                let w = 0.9 / 6.0;
                let x = 1.5 / 6.0 + (i as f32 / 6.0);
                let h = if n.l == 0 {
                    chip_h
                } else {
                    (n.l as f32) / y_view_div
                };
                let yoff = (view_tick - n.y as i64) as f32;
                let y = yoff / y_view_div - h;
                let _p = if n.l == 0 { 2 } else { 1 }; //sorting priority
                notes.push((
                    vec3(x, y, 0.0),
                    vec2(w, h),
                    if n.l > 0 {
                        NoteType::BtHold
                    } else {
                        NoteType::BtChip
                    },
                ));
            }
        }
        for i in 0..2 {
            for n in &chart.note.fx[i] {
                if (n.y as i64) > last_view_tick {
                    break;
                } else if ((n.y + n.l) as i64) < first_view_tick {
                    continue;
                }
                let w = 1.0 / 3.0;
                let x = 1.0 / 3.0 + (1.0 / 3.0) * i as f32;
                let h = if n.l == 0 {
                    chip_h
                } else {
                    (n.l as f32) / y_view_div
                };
                let yoff = (view_tick - n.y as i64) as f32;
                let y = yoff / y_view_div - h;
                let _p = if n.l == 0 { 3 } else { 0 }; //sorting priority
                notes.push((
                    vec3(x, y, 0.0),
                    vec2(w, h),
                    if n.l > 0 {
                        NoteType::FxHold
                    } else {
                        NoteType::FxChip
                    },
                ));
            }
        }

        let notes = notes
            .iter()
            .map(|n| (xy_rect(n.0 - vec3(0.5, n.1.y / -2.0, 0.0), n.1), n.2));

        let mut fx_hold = xy_rect(Vec3::zero(), Vec2::zero());
        let mut fx_hold_active = xy_rect(Vec3::zero(), Vec2::zero());
        let mut bt_hold = xy_rect(Vec3::zero(), Vec2::zero());
        let mut bt_hold_active = xy_rect(Vec3::zero(), Vec2::zero());
        let mut fx_chip = xy_rect(Vec3::zero(), Vec2::zero());
        let mut fx_chip_sample = xy_rect(Vec3::zero(), Vec2::zero());
        let mut bt_chip = xy_rect(Vec3::zero(), Vec2::zero());
        let mut lasers = [
            xy_rect(Vec3::zero(), Vec2::zero()),
            xy_rect(Vec3::zero(), Vec2::zero()),
            xy_rect(Vec3::zero(), Vec2::zero()),
            xy_rect(Vec3::zero(), Vec2::zero()),
        ];

        for n in notes {
            match n.1 {
                NoteType::BtChip => bt_chip = extend_mesh(bt_chip, n.0),
                NoteType::BtHold => bt_hold = extend_mesh(bt_hold, n.0),
                NoteType::BtHoldActive => bt_hold_active = extend_mesh(bt_hold_active, n.0),
                NoteType::FxChip => fx_chip = extend_mesh(fx_chip, n.0),
                NoteType::FxChipSample => fx_chip_sample = extend_mesh(fx_chip_sample, n.0),
                NoteType::FxHold => fx_hold = extend_mesh(fx_hold, n.0),
                NoteType::FxHoldActive => fx_hold_active = extend_mesh(fx_hold_active, n.0),
            }
        }

        //lasers
        {
            for i in 0..2 {
                for (sidx, s) in chart.note.laser[i].iter().enumerate() {
                    let end_y = s.tick() + s.last().unwrap().ry;
                    if (s.tick() as i64) > last_view_tick {
                        break;
                    } else if (end_y as i64) < first_view_tick {
                        continue;
                    }
                    let vertices = self.laser_meshes[i].get(sidx).unwrap();
                    let yoff = (view_tick - s.tick() as i64) as f32;
                    let laser_mesh = CpuMesh {
                        indices: Indices::U32((0u32..(vertices.len() as u32)).collect()),
                        positions: three_d::Positions::F32(
                            vertices
                                .iter()
                                .map(|v| vec3(v.pos.z, (yoff - v.pos.x) / y_view_div, v.pos.y))
                                .collect(),
                        ),
                        uvs: Some(vertices.iter().map(|v| vec2(v.uv.x, v.uv.y)).collect()),
                        ..Default::default()
                    };

                    let active = 0;
                    let extending = std::mem::take(&mut lasers[i * 2 + active]);
                    let extended = extend_mesh(extending, laser_mesh);
                    lasers[i * 2 + active] = extended;
                }
            }
        }
        TrackRenderMeshes {
            fx_hold,
            fx_hold_active,
            bt_hold,
            bt_hold_active,
            fx_chip,
            fx_chip_sample,
            bt_chip,
            lasers,
        }
    }
}

#[derive(Debug, Serialize, Default, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
struct LuaGameState {
    title: String,
    artist: String,
    jacket_path: PathBuf,
    demo_mode: bool,
    difficulty: u8,
    level: u8,
    progress: f32, // 0.0 at the start of a song, 1.0 at the end
    hispeed: f32,
    hispeed_adjust: u32, // 0 = not adjusting, 1 = coarse (xmod) adjustment, 2 = fine (mmod) adjustment
    bpm: f32,
    gauge: LuaGauge,
    hidden_cutoff: f32,
    sudden_cutoff: f32,
    hidden_fade: f32,
    sudden_fade: f32,
    autoplay: bool,
    combo_state: u32,                // 2 = puc, 1 = uc, 0 = normal
    note_held: [bool; 6], // Array indicating wether a hold note is being held, in order: ABCDLR
    laser_active: [bool; 2], // Array indicating if the laser cursor is on a laser, in order: LR
    score_replays: Vec<ScoreReplay>, //Array of previous scores for the current song
    crit_line: CritLine,  // info about crit line and everything attached to it
    hit_window: HitWindow, // This may be absent (== nil) for the default timing window (46 / 92 / 138 / 250ms)
    multiplayer: bool,
    user_id: String,
    practice_setup: bool, // true: it's the setup, false: practicing n
}

#[derive(Debug, Serialize, Default, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
struct LuaGauge {
    #[serde(rename = "type")]
    gauge_type: i32,
    options: i32,
    value: f32,
    name: String,
}

#[derive(Debug, Serialize, Default, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HitWindow {
    #[serde(rename = "type")]
    variant: i32,
    perfect: f64,
    good: f64,
    hold: f64,
    miss: f64,
}

#[derive(Debug, Serialize, Default, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
struct CritLine {
    x: i32,
    y: i32,
    rotation: f32,
    cursors: [Cursor; 2],
    line: Line,
    x_offset: f32,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq)]
struct Cursor {
    pos: f32,
    alpha: f32,
    skew: f32,
}

impl Cursor {
    pub fn new(pos: f32, camera: &Camera, alpha: f32) -> Self {
        let pos = (pos - 0.5) * (5.0 / 6.0);

        let crit_pos = Vec2::from(camera.pixel_at_position(vec3(0.0, 0.0, 0.0)));
        let c_pos = Vec2::from(camera.pixel_at_position(vec3(pos, 0.0, 0.0)));
        let c_pos_up = Vec2::from(camera.pixel_at_position(vec3(pos, 1.0, 0.0)));
        let c_pos_down = Vec2::from(camera.pixel_at_position(vec3(pos, -1.0, 0.0)));
        let dist_from_crit_center =
            (crit_pos - c_pos).magnitude() * if pos < 0.0 { -1.0 } else { 1.0 };
        let cursor_angle_vector = c_pos_up - c_pos_down;

        let skew =
            -cursor_angle_vector.y.atan2(cursor_angle_vector.x) + std::f32::consts::FRAC_PI_2;

        Self {
            pos: dist_from_crit_center,
            alpha,
            skew,
        }
    }
}

#[derive(Debug, Serialize, Default, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Line {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
}

#[derive(Debug, Serialize, Default, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ScoreReplay {
    max_score: i32,
    current_score: i32,
}

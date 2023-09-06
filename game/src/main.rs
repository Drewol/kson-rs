use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{mpsc::channel, Arc, Mutex, RwLock},
    time::Duration,
};

use crate::{
    button_codes::{LaserState, RuscFilter},
    config::Args,
    config::GameConfig,
    game::GameData,
    game_main::GameMain,
    input_state::InputState,
    scene::SceneData,
    skin_settings::SkinSettingEntry,
    songselect::{Difficulty, Song},
    transition::Transition,
    vg_ui::Vgfx,
};
use anyhow::bail;
use clap::Parser;
use directories::ProjectDirs;
use egui_glow::EguiGlow;
use femtovg as vg;

use game_main::ControlMessage;
use generational_arena::{Arena, Index};
use gilrs::ev::filter::Jitter;

use kson::Ksh;
use log::*;

use puffin::profile_function;
use rodio::{dynamic_mixer::DynamicMixerController, Source};
use scene::Scene;

use td::{FrameInput, HasContext, Viewport};
use tealr::mlu::mlua::Lua;
use three_d as td;

use glutin::prelude::*;

mod animation;
mod audio;
mod audio_test;
mod button_codes;
mod config;
mod game;
mod game_background;
mod game_camera;
mod game_data;
mod game_main;
mod help;
mod input_state;
mod lua_http;
mod main_menu;
mod material;
mod results;
mod scene;
mod settings_dialog;
mod settings_screen;
mod shaded_mesh;
mod skin_settings;
mod song_provider;
mod songselect;
mod sources;
mod take_duration_fade;
mod transition;
mod util;
mod vg_ui;
mod window;

#[macro_export]
macro_rules! block_on {
    ($l:expr) => {
        poll_promise::Promise::spawn_async(async move {
            let x = { $l };
            x.await
        })
        .block_and_take()
    };
}

pub type RuscMixer = Arc<DynamicMixerController<f32>>;

//TODO: Move to platform files
#[cfg(target_os = "windows")]
pub fn default_game_dir() -> PathBuf {
    let mut game_dir = directories::UserDirs::new()
        .expect("Failed to get directories")
        .document_dir()
        .expect("Failed to get documents directory")
        .to_path_buf();
    game_dir.push("USC");
    game_dir
}
#[cfg(not(target_os = "windows"))]
pub fn default_game_dir() -> PathBuf {
    let mut game_dir = directories::UserDirs::new()
        .expect("Failed to get directories")
        .home_dir()
        .to_path_buf();
    game_dir.push(".usc");
    game_dir
}

pub fn init_game_dir(game_dir: impl AsRef<Path>) -> anyhow::Result<()> {
    let mut install_dir = std::env::current_dir()?;
    install_dir.push("fonts");

    if !install_dir.exists() {
        install_dir = std::env::current_exe()?;
        install_dir.pop();
        install_dir.push("fonts");

        if !install_dir.exists() {
            bail!("Could not find installed assets.")
        }
    }

    std::fs::create_dir_all(&game_dir)?;
    install_dir.pop();
    let r = install_dir.read_dir()?;
    for ele in r.into_iter() {
        let ele = ele?;
        let folder_name = ele.file_name().into_string().unwrap();
        if ele.file_type()?.is_dir() && (folder_name == "fonts" || folder_name == "skins") {
            for data_file in walkdir::WalkDir::new(ele.path()).into_iter() {
                let data_file = data_file?;

                let target_file = data_file.path().strip_prefix(&install_dir)?;
                let mut target_path = game_dir.as_ref().to_path_buf();
                target_path.push(target_file);

                if data_file.file_type().is_dir() {
                    std::fs::create_dir_all(target_path)?;
                    continue;
                }

                info!("Installing: {:?} -> {:?}", data_file.path(), &target_path);
                std::fs::copy(data_file.path(), target_path)?;
            }
        }
    }

    Ok(())
}

pub fn project_dirs() -> ProjectDirs {
    directories::ProjectDirs::from("", "Drewol", "USC").expect("Failed to get project dirs")
}

pub struct Scenes {
    pub active: Vec<Box<dyn Scene>>,
    pub loaded: Vec<Box<dyn Scene>>,
    pub initialized: Vec<Box<dyn Scene>>,
    pub transition: Option<Transition>,
    pub prev_transition: bool,
    should_outro: bool,
    mixer: RuscMixer,
}

impl Scenes {
    pub fn new(mixer: Arc<DynamicMixerController<f32>>) -> Self {
        Self {
            active: Default::default(),
            loaded: Default::default(),
            initialized: Default::default(),
            transition: Default::default(),
            should_outro: Default::default(),
            mixer,
            prev_transition: false,
        }
    }

    pub fn tick(
        &mut self,
        dt: f64,
        knob_state: crate::button_codes::LaserState,
        load_lua: Rc<dyn Fn(Rc<Lua>, &'static str) -> anyhow::Result<Index>>,
        app_control_tx: std::sync::mpsc::Sender<ControlMessage>,
    ) {
        let new_transition = self.transition.is_some();
        if self.should_outro {
            if let Some(tr) = self.transition.as_mut() {
                tr.do_outro()
            }

            self.should_outro = false;
        }

        self.active.retain(|x| !x.closed());
        if let Some(t) = self.transition.as_mut() {
            t.tick(dt, knob_state);
        }

        if self.transition.is_some() && self.transition.as_ref().unwrap().closed() {
            self.transition = None;
        }

        for ele in &mut self.active {
            ele.tick(dt, knob_state);
        }

        if !self.initialized.is_empty() {
            for scene in &mut self.active {
                scene.suspend();
            }

            self.should_outro = true;
        }

        self.active.append(&mut self.initialized);

        self.loaded.retain_mut(|x| {
            let result = x.init(load_lua.clone(), app_control_tx.clone(), self.mixer.clone());
            if let Err(e) = &result {
                log::error!("{}", e);
            }
            result.is_ok()
        });

        if !self.prev_transition && new_transition {
            if let Some(top) = self.active.last_mut() {
                top.suspend();
            }
        }

        self.initialized.append(&mut self.loaded);

        if let Some(x) = self.active.last_mut() {
            if x.is_suspended()
                && self.loaded.is_empty()
                && self.initialized.is_empty()
                && self.transition.is_none()
            {
                x.resume()
            }
        }

        self.prev_transition = new_transition;
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
            && self.loaded.is_empty()
            && self.initialized.is_empty()
            && self.transition.is_none()
    }

    pub fn render(&mut self, frame: FrameInput<()>, _vgfx: &Arc<Mutex<Vgfx>>) {
        profile_function!();
        let dt = frame.elapsed_time;
        let td_context = &frame.context;
        let mut target = frame.screen();
        let viewport = frame.viewport;

        for scene in &mut self.active {
            if scene.is_suspended() {
                continue;
            }
            scene.render(dt, td_context, &mut target, viewport);
            if let Err(e) = scene.render_ui(dt) {
                log::error!("{}", e)
            };
        }

        if let Some(transition) = self.transition.as_mut() {
            transition.render(dt, td_context, &mut target, viewport);
            if let Err(e) = transition.render_ui(dt) {
                log::error!("{}", e)
            };
        }
    }

    pub fn should_render_egui(&self) -> bool {
        self.active.iter().any(|s| s.has_egui())
    }

    pub fn render_egui(&mut self, ctx: &egui::Context) {
        profile_function!();
        for scene in &mut self.active {
            if scene.is_suspended() {
                continue;
            }
            scene.render_egui(ctx);
        }
    }

    pub fn suspend_top(&mut self) {
        if let Some(top) = self.active.last_mut() {
            top.suspend()
        }
    }

    pub fn for_each_active_mut(&mut self, f: impl FnMut(&mut Box<dyn Scene>)) {
        self.active
            .iter_mut()
            .filter(|x| !x.is_suspended())
            .for_each(f);
    }

    pub fn clear(&mut self) {
        self.active.clear();
        self.initialized.clear();
        self.loaded.clear();
        self.transition = None;
    }
}
pub const FRAME_ACC_SIZE: usize = 16;

fn main() -> anyhow::Result<()> {
    simple_logger::init_with_level(Level::Info)?;

    let mut config_path = default_game_dir();
    config_path.push("Main.cfg");
    let args = Args::parse();
    let show_debug_ui = args.debug;
    if let Some(e) = init_game_dir(default_game_dir()).err() {
        warn!("{e}");
        info!("Running anyway");
    };
    GameConfig::init(config_path, args);
    let (_outputStream, outputStreamHandle) = rodio::OutputStream::try_default()?;
    let sink = rodio::Sink::try_new(&outputStreamHandle)?;
    let (mixer_controls, mixer) = rodio::dynamic_mixer::mixer::<f32>(2, 44100);
    mixer_controls.add(rodio::source::Zero::new(2, 44100));
    sink.append(mixer);
    sink.play();
    let (killed, _killer) = channel();

    for _ in 0..(num_cpus::get() / 2).max(1) {
        let killed = killed.clone();
        let _async = std::thread::spawn(move || loop {
            poll_promise::tick();
            std::thread::sleep(Duration::from_millis(1));
            if killed.send(()).is_err() {
                return;
            }
        });
    }

    let (window, surface, canvas, gl_context, eventloop, window_gl) = window::create_window();

    {
        if GameConfig::get().mouse_knobs {
            window.set_cursor_visible(false)
        }
    }

    let gl_context = Arc::new(gl_context);
    let canvas = Arc::new(Mutex::new(canvas));

    let skin_setting: Vec<SkinSettingEntry> = {
        let skin = &GameConfig::get().skin;
        let mut config_def_path = default_game_dir();
        config_def_path.push("skins");
        config_def_path.push(skin);
        config_def_path.push("config-definitions.json");
        let res = File::open(config_def_path).map(|f| {
            let res = serde_json::from_reader::<_, Vec<SkinSettingEntry>>(f);
            if let Err(e) = &res {
                log::error!("{:?}", e);
            }

            res.unwrap_or_default()
        });

        if let Err(e) = &res {
            log::error!("{:?}", e);
        }

        res.unwrap_or_default()
    };

    log::info!("Skin Settings: {:#?}", skin_setting);

    let mut input = gilrs::GilrsBuilder::default()
        .add_included_mappings(false)
        .with_default_filters(false)
        .add_mappings(&GameConfig::get().mappings.join("\n"))
        .build()
        .expect("Failed to create input context");

    while input.next_event().is_some() {} //empty events

    let context = td::Context::from_gl_context(gl_context.clone())?;
    let vgfx = Arc::new(Mutex::new(vg_ui::Vgfx::new(
        canvas.clone(),
        default_game_dir(),
    )));

    input
        .gamepads()
        .for_each(|(_, g)| info!("{} uuid: {}", g.name(), uuid::Uuid::from_bytes(g.uuid())));

    let input = Arc::new(Mutex::new(input));
    let gilrs_state = input.clone();
    let input_state = InputState::new(gilrs_state);

    let mousex = 0.0;
    let mousey = 0.0;

    let event_proxy = eventloop.create_proxy();

    let _input_thread = poll_promise::Promise::spawn_thread("gilrs", move || {
        let mut knob_state = LaserState::default();
        let rusc_filter = RuscFilter::new();
        loop {
            use button_codes::*;
            use game_loop::winit::event::ElementState::*;
            use gilrs::*;
            let e = {
                if let Ok(mut input) = input.lock() {
                    input.next_event().filter_ev(&rusc_filter, &mut input)
                } else {
                    None
                }
            };
            knob_state.zero_deltas();
            if let Some(e) = e {
                let sent = match e.event {
                    EventType::ButtonPressed(button, _) => {
                        let button = UscButton::from(button);
                        info!("{:?}", button);
                        Some(event_proxy.send_event(UscInputEvent::Button(button, Pressed)))
                    }
                    EventType::ButtonRepeated(_, _) => None,
                    EventType::ButtonReleased(button, _) => {
                        let button = UscButton::from(button);
                        info!("{:?}", button);
                        Some(event_proxy.send_event(UscInputEvent::Button(button, Released)))
                    }
                    EventType::ButtonChanged(_, _, _) => None,
                    EventType::AxisChanged(axis, value, _) => {
                        match axis {
                            Axis::LeftStickX => knob_state.update(kson::Side::Left, value),
                            Axis::RightStickX => knob_state.update(kson::Side::Right, value),
                            e => {
                                info!("{:?}", e)
                            }
                        }
                        Some(event_proxy.send_event(UscInputEvent::Laser(knob_state)))
                    }
                    EventType::Connected => None,
                    EventType::Disconnected => None,
                    EventType::Dropped => None,
                };

                if let Some(Err(send_err)) = sent {
                    info!("Gilrs thread closing: {}", send_err);
                    return;
                }
            } else {
                std::thread::sleep(Duration::from_millis(1))
            }
        }
    });

    let mut typedef_folder = default_game_dir();
    typedef_folder.push("types");
    if !typedef_folder.exists() {
        std::fs::create_dir_all(&typedef_folder)?;
    }

    let gfx_typedef = tealr::TypeWalker::new()
        .process_type_inline::<vg_ui::Vgfx>()
        .generate_global("gfx")?;

    let game_typedef = tealr::TypeWalker::new()
        .process_type_inline::<game_data::GameData>()
        .generate_global("game")?;

    let songwheel_typedef = tealr::TypeWalker::new()
        .process_type::<songselect::Song>()
        .process_type::<songselect::Difficulty>()
        .process_type_inline::<songselect::SongSelect>()
        .generate_global("songwheel")?;

    let mut typedef_file_path = typedef_folder;
    typedef_file_path.push("rusc.d.tl");
    let mut typedef_file = std::fs::File::create(typedef_file_path).expect("Failed to create");
    let file_content = format!("{}\n{}\n{}", gfx_typedef, game_typedef, songwheel_typedef)
        .lines()
        .filter(|l| !l.starts_with("return"))
        .collect::<Vec<_>>()
        .join("\n");

    write!(typedef_file, "{}", file_content)?;
    typedef_file.flush()?;
    drop(typedef_file);
    let gui = egui_glow::EguiGlow::new(&eventloop, gl_context, None);

    let frame_times = [16.0; FRAME_ACC_SIZE];
    let frame_time_index = 0;

    let fps_paint = vg::Paint::color(vg::Color::white()).with_text_align(vg::Align::Right);

    let game_data = Arc::new(Mutex::new(game_data::GameData {
        mouse_pos: (mousex, mousey),
        resolution: (800, 600),
        profile_stack: vec![],
        laser_state: LaserState::default(),
        audio_sample_play_status: Default::default(),
        audio_samples: Default::default(),
    }));

    let mut scenes = Scenes::new(mixer_controls.clone());
    if GameConfig::get().args.chart.as_ref().is_none() {
        let mut title = Box::new(main_menu::MainMenu::new());
        title.suspend();
        scenes.loaded.push(title);
    }

    if let Some(chart_path) = GameConfig::get().args.chart.as_ref() {
        let chart_path = PathBuf::from(chart_path);
        let chart =
            kson::Chart::from_ksh(&std::io::read_to_string(std::fs::File::open(&chart_path)?)?)?;

        let song = Song {
            title: chart.meta.title.clone(),
            artist: chart.meta.artist.clone(),
            bpm: chart.meta.disp_bpm.clone(),
            id: 0,
            difficulties: vec![Difficulty {
                jacket_path: chart_path.with_file_name(&chart.meta.jacket_filename),
                level: chart.meta.level,
                difficulty: chart.meta.difficulty,
                id: 0,
                effector: chart.meta.chart_author.clone(),
                top_badge: 0,
                hash: None,
                scores: vec![],
            }],
        };

        let audio = rodio::Decoder::new(std::fs::File::open(
            chart_path.with_file_name(chart.audio.bgm.as_ref().unwrap().filename.clone().unwrap()),
        )?)?;

        let skin_folder = { vgfx.lock().unwrap().skin_folder() };

        scenes.loaded.push(
            Box::new(GameData::new(
                context.clone(),
                Arc::new(song),
                0,
                chart,
                skin_folder,
                Box::new(audio.convert_samples()),
            )?)
            .make_scene(input_state.clone(), vgfx.clone(), game_data.clone()),
        );
    }

    if GameConfig::get().args.sound_test {
        scenes
            .loaded
            .push(Box::new(audio_test::AudioTest::new(mixer_controls.clone())));
    }

    let lua_arena: Rc<RwLock<Arena<Rc<Lua>>>> = Rc::new(RwLock::new(Arena::new()));

    let transition_lua_idx = Index::from_raw_parts(0, 0);
    let transition_song_lua_idx = Index::from_raw_parts(0, 0);

    let _jitter_filter = Jitter { threshold: 0.005 };
    let knob_state = LaserState::default();

    let (control_tx, control_rx) = std::sync::mpsc::channel();
    let _second_frame = false;

    let game = GameMain::new(
        lua_arena,
        scenes,
        control_tx,
        control_rx,
        knob_state,
        frame_times,
        frame_time_index,
        fps_paint,
        transition_lua_idx,
        transition_song_lua_idx,
        game_data,
        vgfx,
        canvas,
        0,
        gui,
        show_debug_ui,
        mousex,
        mousey,
        input_state,
        mixer_controls,
    );

    game_loop::game_loop(
        eventloop,
        Arc::new(window),
        game,
        240,
        0.1,
        move |g| g.game.update(),
        move |g| {
            let frame_out = g.game.render(
                FrameInput {
                    events: vec![],
                    elapsed_time: g.last_frame_time() * 1000.0,
                    accumulated_time: g.accumulated_time() * 1000.0,
                    viewport: Viewport {
                        x: 0,
                        y: 0,
                        width: g.window.inner_size().width,
                        height: g.window.inner_size().height,
                    },
                    window_width: g.window.outer_size().width,
                    window_height: g.window.outer_size().height,
                    device_pixel_ratio: 1.0,
                    first_frame: g.number_of_renders() == 0,
                    context: context.clone(),
                },
                &g.window,
            );
            surface.swap_buffers(&window_gl);

            if frame_out.exit {
                g.exit()
            }
        },
        move |g, e| g.game.handle(&g.window, e),
    );
}

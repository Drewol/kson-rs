use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use crate::{
    button_codes::{LaserState, RuscFilter},
    config::Args,
    config::GameConfig,
    game_main::GameMain,
    input_state::InputState,
    scene::SceneData,
    songselect::{Difficulty, Song},
    transition::Transition,
    vg_ui::Vgfx,
};
use anyhow::{anyhow, bail};
use button_codes::CustomBindingFilter;
use clap::Parser;
use directories::ProjectDirs;

use femtovg as vg;

use game_main::ControlMessage;

use glutin_winit::GlWindow;
use help::ServiceHelper;
use kson::Ksh;
use log::*;

use lua_service::LuaProvider;
use puffin::profile_function;
use rodio::{dynamic_mixer::DynamicMixerController, Source};
use scene::Scene;

use song_provider::{DiffId, FileSongProvider, NauticaSongProvider, SongId};
use td::{FrameInput, Viewport};
use tealr::mlu::mlua::Lua;
use three_d as td;

use di::*;
use glutin::prelude::*;

mod animation;
mod audio;
mod audio_test;
mod button_codes;
mod config;
mod game;
mod game_data;
mod game_main;
mod help;
mod input_state;
mod lua_http;
mod lua_service;
mod main_menu;
mod results;
mod scene;
mod settings_dialog;
mod settings_screen;
mod shaded_mesh;
mod skin_settings;
mod song_provider;
mod songselect;
mod take_duration_fade;
mod transition;
mod util;
mod vg_ui;
mod window;
mod worker_service;

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

pub type InnerRuscMixer = DynamicMixerController<f32>;
pub type RuscMixer = Arc<InnerRuscMixer>;

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
    let cargo_dir = std::env::var("CARGO_MANIFEST_DIR");

    let mut install_dir = if let Ok(manifest_dir) = &cargo_dir {
        PathBuf::from(manifest_dir) // should be correct when started from `cargo run`
    } else {
        std::env::current_dir()?
    };

    install_dir.push("fonts");

    if !install_dir.exists() {
        install_dir = std::env::current_exe()?;
        install_dir.pop();
        #[cfg(target_os = "macos")]
        {
            //if app bundle
            if install_dir.with_file_name("Resources").exists() {
                install_dir.set_file_name("Resources");
            }
        }

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
        let folder_name = ele
            .file_name()
            .into_string()
            .map_err(|_| anyhow!("Bad file name"))?;

        if ele.file_type()?.is_dir() && (folder_name == "fonts" || folder_name == "skins") {
            // Quickly check if the root path exists, ignore it if it does
            let path = ele.path();
            let target = path.strip_prefix(&install_dir)?;
            let mut target_path = game_dir.as_ref().to_path_buf();
            target_path.push(target);

            // Always install when cargo in cargo for easier skin dev
            if target_path.exists() && cargo_dir.is_err() {
                continue;
            }

            for data_file in walkdir::WalkDir::new(path).into_iter() {
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
}

impl Scenes {
    pub fn new() -> Self {
        Self {
            active: Default::default(),
            loaded: Default::default(),
            initialized: Default::default(),
            transition: Default::default(),
            should_outro: Default::default(),
            prev_transition: false,
        }
    }

    pub fn tick(
        &mut self,
        dt: f64,
        knob_state: crate::button_codes::LaserState,
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
            log_result!(t.tick(dt, knob_state));
        }

        if self.transition.as_ref().is_some_and(|x| x.closed()) {
            self.transition = None;
        }

        for ele in &mut self.active {
            log_result!(ele.tick(dt, knob_state));
        }

        if !self.initialized.is_empty() {
            for scene in &mut self.active {
                scene.suspend();
            }

            self.should_outro = true;
        }

        self.active.append(&mut self.initialized);

        self.loaded.retain_mut(|x| {
            let result = x.init(app_control_tx.clone());
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

    pub fn render(&mut self, frame: FrameInput, _vgfx: &Arc<RwLock<Vgfx>>) {
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
            log_result!(scene.render_egui(ctx));
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

impl Default for Scenes {
    fn default() -> Self {
        Self::new()
    }
}
pub const FRAME_ACC_SIZE: usize = 16;

struct LuaArena(Vec<Rc<Lua>>);

fn main() -> anyhow::Result<()> {
    simple_logger::init_with_level(Level::Info)?;

    let mut config_path = default_game_dir();
    config_path.push("Main.cfg");
    let args = Args::parse();

    let _puffin_server = if args.profiling {
        let server_addr = format!("127.0.0.1:{}", puffin_http::DEFAULT_PORT);
        Some(puffin_http::Server::new(&server_addr)?)
    } else {
        None
    };

    puffin::set_scopes_on(args.profiling);

    let show_debug_ui = args.debug;
    if let Some(e) = init_game_dir(default_game_dir()).err() {
        warn!("{e}");
        info!("Running anyway");
    };
    GameConfig::init(config_path, args);
    let (_output_stream, output_stream_handle) = rodio::OutputStream::try_default()?;
    let sink = rodio::Sink::try_new(&output_stream_handle)?;
    let (mixer_controls, mixer) = rodio::dynamic_mixer::mixer::<f32>(2, 44100);
    mixer_controls.add(rodio::source::Zero::new(2, 44100));
    sink.append(mixer);
    sink.play();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let _tokio = rt.enter();

    let (window, surface, canvas, gl_context, eventloop, window_gl) = window::create_window()?;

    {
        if GameConfig::get().mouse_knobs {
            window.set_cursor_visible(false)
        }
    }

    let gl_context = Arc::new(gl_context);

    let mut input = gilrs::GilrsBuilder::default()
        .add_included_mappings(false)
        .with_default_filters(false)
        .add_mappings(&GameConfig::get().mappings.join("\n"))
        .build()
        .expect("Failed to create input context");

    while input.next_event().is_some() {} //empty events

    let context = td::Context::from_gl_context(gl_context.clone())?;

    input
        .gamepads()
        .for_each(|(_, g)| info!("{} uuid: {}", g.name(), uuid::Uuid::from_bytes(g.uuid())));
    let input = Arc::new(Mutex::new(input));
    let gilrs_state = input.clone();
    let service_context = context.clone();

    let services = ServiceCollection::new()
        .add(existing_as_self(Mutex::new(canvas)))
        .add(existing_as_self(service_context.clone()))
        .add(singleton_factory(|_| {
            RefMut::new(block_on!(song_provider::FileSongProvider::new()).into())
        }))
        .add(singleton_factory(|_| {
            RefMut::new(song_provider::NauticaSongProvider::new().into())
        }))
        .add(transient_factory::<
            RwLock<dyn song_provider::SongProvider>,
            _,
        >(|sp| {
            if GameConfig::get().songs_path.eq(&PathBuf::from("nautica")) {
                sp.get_required_mut::<song_provider::NauticaSongProvider>()
            } else {
                sp.get_required_mut::<song_provider::FileSongProvider>()
            }
        }))
        .add(transient_factory::<
            RwLock<dyn song_provider::ScoreProvider>,
            _,
        >(|sp| {
            sp.get_required_mut::<song_provider::FileSongProvider>()
        }))
        .add_worker::<FileSongProvider>()
        .add_worker::<NauticaSongProvider>()
        .add(singleton_factory(move |_| mixer_controls.clone()))
        .add(Vgfx::singleton().as_mut())
        .add(singleton_factory(|_| {
            RefMut::new(LuaArena(Vec::new()).into())
        }))
        .add(singleton_factory(move |_| {
            Arc::new(InputState::new(gilrs_state.clone()))
        }))
        .add(game_data::GameData::singleton().as_mut())
        .add(LuaProvider::scoped())
        .build_provider()?;

    let _mousex = 0.0;
    let _mousey = 0.0;
    let _lua_provider: Arc<LuaProvider> = services.get_required();
    let vgfx = services.get_required_mut::<Vgfx>();
    let event_proxy = eventloop.create_proxy();
    let (mut rusc_filter, offset_tx) = RuscFilter::new(GameConfig::get().global_offset as _);

    let _input_thread = poll_promise::Promise::spawn_thread("gilrs", move || {
        let mut knob_state = LaserState::default();
        let binding_filter = CustomBindingFilter;
        loop {
            rusc_filter.update();
            use button_codes::*;
            use game_loop::winit::event::ElementState::*;
            use gilrs::*;
            let e = {
                if let Ok(mut input) = input.lock() {
                    input
                        .next_event()
                        .filter_ev(&rusc_filter, &mut input)
                        .filter_ev(&binding_filter, &mut input)
                } else {
                    None
                }
            };
            knob_state.zero_deltas();
            if let Some(e) = e {
                let sent = match e.event {
                    EventType::ButtonPressed(button, _) => {
                        let button = UscButton::from(button);
                        info!("Pressed {:?}", button);
                        Some(event_proxy.send_event(UscInputEvent::Button(button, Pressed, e.time)))
                    }
                    EventType::ButtonRepeated(_, _) => None,
                    EventType::ButtonReleased(button, _) => {
                        let button = UscButton::from(button);
                        info!("Released {:?}", button);
                        Some(
                            event_proxy.send_event(UscInputEvent::Button(button, Released, e.time)),
                        )
                    }
                    EventType::ButtonChanged(_, _, _) => None,
                    EventType::AxisChanged(axis, value, _) => {
                        match axis {
                            Axis::LeftStickX => knob_state.update(kson::Side::Left, value),
                            Axis::RightStickX => knob_state.update(kson::Side::Right, value),
                            _ => {}
                        }
                        Some(event_proxy.send_event(UscInputEvent::Laser(knob_state, e.time)))
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

    //TODO: Export tealr types, or move to some other typed lua

    let gui = egui_glow::EguiGlow::new(&eventloop, gl_context, None, None);

    let _frame_times = [16.0; FRAME_ACC_SIZE];
    let _frame_time_index = 0;

    let fps_paint = vg::Paint::color(vg::Color::white()).with_text_align(vg::Align::Right);

    let mut scenes = Scenes::new();

    if GameConfig::get().args.chart.as_ref().is_none() {
        let mut title = Box::new(main_menu::MainMenu::new(services.create_scope()));
        title.suspend();
        scenes.loaded.push(title);
        if GameConfig::get().args.notitle {
            let songsel = Box::new(songselect::SongSelectScene::new(
                Box::new(songselect::SongSelect::new()),
                services.create_scope(),
            ));
            scenes.loaded.push(songsel);
        }
    }

    if let Some(chart_path) = GameConfig::get().args.chart.as_ref() {
        let chart_path = PathBuf::from(chart_path);
        let chart =
            kson::Chart::from_ksh(&std::io::read_to_string(std::fs::File::open(&chart_path)?)?)?;

        let song = Song {
            title: chart.meta.title.clone(),
            artist: chart.meta.artist.clone(),
            bpm: chart.meta.disp_bpm.clone(),
            id: SongId::default(),
            difficulties: Arc::new(
                vec![Difficulty {
                    jacket_path: chart_path.with_file_name(&chart.meta.jacket_filename),
                    level: chart.meta.level,
                    difficulty: chart.meta.difficulty,
                    id: DiffId::default(),
                    effector: chart.meta.chart_author.clone(),
                    top_badge: 0,
                    hash: None,
                    scores: vec![],
                }]
                .into(),
            ),
        };

        let audio = rodio::Decoder::new(std::fs::File::open(
            chart_path.with_file_name(chart.audio.bgm.filename.clone()),
        )?)?;

        let skin_folder = { vgfx.read().expect("Lock error").skin_folder() };

        scenes.loaded.push(
            Box::new(game::GameData::new(
                Arc::new(song),
                0,
                chart,
                skin_folder,
                Box::new(audio.convert_samples()),
            )?)
            .make_scene(services.create_scope())?,
        );
    }

    if GameConfig::get().args.sound_test {
        scenes.loaded.push(Box::new(audio_test::AudioTest::new(
            services.create_scope(),
        )));
    }

    let game = GameMain::new(scenes, fps_paint, gui, show_debug_ui, services);

    let mut last_offset = { GameConfig::get().global_offset };

    game_loop::game_loop(
        eventloop,
        Arc::new(window),
        game,
        240,
        0.1,
        move |g| g.game.update(),
        move |g| {
            // Check for offset changes
            {
                let current_offset = GameConfig::get().global_offset;
                if current_offset != last_offset {
                    log_result!(offset_tx.send(current_offset));
                    last_offset = current_offset;
                }
            }

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
                &surface,
                &window_gl,
            );
            surface
                .swap_buffers(&window_gl)
                .expect("Failed to swap buffer");

            //TODO: Only do on resize
            g.window.resize_surface(&surface, &window_gl);

            if frame_out.exit {
                g.exit()
            }
        },
        move |g, e| g.game.handle(&g.window, e),
    )?;
    Ok(())
}

#[macro_export]
macro_rules! log_result {
    ($expression:expr) => {
        if let Err(e) = $expression {
            log::warn!("{e}");
        }
    };
}

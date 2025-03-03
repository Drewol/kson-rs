use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::{mpsc::channel, Arc, Mutex, RwLock},
    time::{Duration, Instant},
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
use anyhow::anyhow;
use async_service::AsyncService;
use button_codes::{CustomBindingFilter, UscInputEvent};
use clap::Parser;
use companion_interface::CompanionServer;
use directories::ProjectDirs;

use femtovg as vg;

use game_main::ControlMessage;

use gilrs::Gilrs;
use glutin_winit::GlWindow;
use help::ServiceHelper;
use kson::Ksh;
use log::*;

use lua_service::LuaProvider;
use luals_gen::LuaLsGen;
use puffin::profile_function;
use rodio::{dynamic_mixer::DynamicMixerController, Source};
use scene::Scene;

use mlua::Lua;
pub(crate) use song_provider::{DiffId, FileSongProvider, NauticaSongProvider, SongId};
use td::{FrameInput, Viewport};
use test_scenes::camera_test;
use three_d as td;

use di::*;
use glutin::{context::PossiblyCurrentContext, prelude::*};
use winit::event::WindowEvent;

mod animation;
mod async_service;
mod audio;
mod audio_test;
mod button_codes;
mod companion_interface;
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
mod test_scenes;
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
#[cfg(all(target_os = "windows", not(feature = "portable")))]
pub fn default_game_dir() -> PathBuf {
    let mut game_dir = directories::UserDirs::new()
        .expect("Failed to get directories")
        .document_dir()
        .expect("Failed to get documents directory")
        .to_path_buf();
    game_dir.push("USC");
    game_dir
}

#[cfg(all(target_os = "windows", feature = "portable"))]
pub fn default_game_dir() -> PathBuf {
    let mut game_dir = std::env::current_exe().expect("Could not get exe path");
    game_dir.pop();
    game_dir
}

#[cfg(not(target_os = "windows"))]
pub fn default_game_dir() -> PathBuf {
    let mut game_dir = directories::UserDirs::new()
        .expect("Failed to get directories")
        .home_dir()
        .to_path_buf();
    game_dir.push(".local");
    game_dir.push("share");
    game_dir.push("usc");
    game_dir
}

fn is_install_dir(dir: impl AsRef<Path>) -> Option<PathBuf> {
    let dir = dir.as_ref();
    let font_dir = dir.join("fonts");
    let skin_dir = dir.join("skins");
    if font_dir.exists() && skin_dir.exists() {
        Some(dir.to_path_buf())
    } else {
        None
    }
}

pub fn init_game_dir(game_dir: impl AsRef<Path>) -> anyhow::Result<()> {
    #[cfg(feature = "portable")]
    {
        return Ok(());
    }

    let mut candidates = vec![];

    let cargo_dir = std::env::var("CARGO_MANIFEST_DIR");

    if let Ok(dir) = &cargo_dir {
        candidates.push(PathBuf::from(dir));
    }

    candidates.push(std::env::current_dir()?);

    let mut candidate_dir = std::env::current_exe()?;
    candidate_dir.pop();
    candidates.push(candidate_dir.clone());
    #[cfg(target_os = "macos")]
    {
        //if app bundle
        if candidate_dir.with_file_name("Resources").exists() {
            candidate_dir.set_file_name("Resources");
        }
    }
    #[cfg(target_os = "linux")]
    {
        //deb installs files to usr/lib/rusc/game
        // assume starting at usr/bin after popping exe
        candidate_dir.pop(); // usr
        candidate_dir.push("lib");
        candidate_dir.push("rusc");
        candidate_dir.push("game");
    }

    candidates.push(candidate_dir);

    let install_dir = candidates
        .iter()
        .filter_map(is_install_dir)
        .next()
        .ok_or(anyhow!("Failed to find installed files"))?;

    if install_dir.as_path() == game_dir.as_ref() {
        info!("Running from install dir");
        return Ok(());
    }

    std::fs::create_dir_all(&game_dir)?;

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
        profile_function!();
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

struct UscApp {
    state: Option<(
        GameMain,
        ServiceProvider,
        winit::window::Window,
        glutin::surface::Surface<glutin::surface::WindowSurface>,
        PossiblyCurrentContext,
        td::Context,
    )>,
    frame_tracker: FrameTracker,
    update_tracker: UpdateTracker,
    gilrs: Arc<Mutex<Gilrs>>,
    mixer_controls: RuscMixer,
    sink: Option<rodio::Sink>,
    companion_service: Option<RwLock<CompanionServer>>,
    offset_tx: std::sync::mpsc::Sender<i32>,
}

struct FrameTracker {
    rendered_frames: u64,
    last_render: Instant,
    app_start: Instant,
    last_frame_sec: f64,
    accum_sec: f64,
}
struct UpdateTracker {
    target_time: Instant,
    current_update: Instant,
}

impl UpdateTracker {
    pub const RATE: u64 = 240;
    pub fn set(&mut self) {
        self.target_time = Instant::now();
    }

    pub fn new() -> Self {
        Self {
            current_update: Instant::now(),
            target_time: Instant::now(),
        }
    }
}

impl Iterator for UpdateTracker {
    type Item = ();

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_update < self.target_time {
            self.current_update = self
                .current_update
                .checked_add(Duration::from_nanos(1_000_000_000 / Self::RATE))
                .expect("Could not set update target time");
            Some(())
        } else {
            None
        }
    }
}

impl UscApp {
    fn init(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) -> anyhow::Result<()> {
        if self.state.is_some() {
            return Ok(());
        }

        let (window, surface, canvas, gl_context, window_gl) =
            window::create_window(event_loop).expect("Failed to create window");

        {
            if GameConfig::get().mouse_knobs {
                window.set_cursor_visible(false)
            }
        }
        let companion_service = self.companion_service.take().unwrap();
        let mixer_controls = self.mixer_controls.clone();
        let gl_context = Arc::new(gl_context);
        let context = td::Context::from_gl_context(gl_context.clone())?;
        let service_context = context.clone();
        let gui = egui_glow::EguiGlow::new(event_loop, gl_context.clone(), None, None, false);
        let gilrs_state = self.gilrs.clone();
        let services = ServiceCollection::new()
            .add(existing_as_self(companion_service))
            .add(existing_as_self(self.sink.take().unwrap()))
            .add(AsyncService::singleton().as_mut())
            .add_worker::<AsyncService>()
            .add(existing_as_self(Mutex::new(canvas)))
            .add(existing_as_self(service_context.clone()))
            .add(singleton_factory(|_| {
                RefMut::new(block_on!(song_provider::FileSongProvider::new()).into())
            }))
            .add(singleton_factory(|x| {
                RefMut::new(song_provider::NauticaSongProvider::new(x.get_required_mut()).into())
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
            .add_worker::<companion_interface::CompanionServer>()
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
            .build_provider()
            .expect("Failed to build service provider");

        let _lua_provider: Arc<LuaProvider> = services.get_required();
        let vgfx = services.get_required_mut::<Vgfx>();

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
            let chart = kson::Chart::from_ksh(&std::io::read_to_string(std::fs::File::open(
                &chart_path,
            )?)?)?;

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
                        illustrator: String::new(),
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
                    game_main::AutoPlay::None,
                )?)
                .make_scene(services.create_scope())?,
            );
        }

        if GameConfig::get().args.sound_test {
            scenes.loaded.push(Box::new(audio_test::AudioTest::new(
                services.create_scope(),
            )));
        }

        if GameConfig::get().args.camera_test {
            scenes.loaded.push(Box::new(camera_test::CameraTest::new(
                services.create_scope(),
                GameConfig::get().skin_path(),
            )))
        }
        let service_scope = services.create_scope();

        if GameConfig::get().args.settings {
            scenes
                .loaded
                .push(Box::new(settings_screen::SettingsScreen::new(
                    service_scope,
                    channel().0,
                    &window,
                )));
        }

        let last_offset = { GameConfig::get().global_offset };
        let fps_paint = vg::Paint::color(vg::Color::white()).with_text_align(vg::Align::Right);

        let game = GameMain::new(
            scenes,
            fps_paint,
            gui,
            GameConfig::get().args.debug,
            services.create_scope(),
        );

        self.state = Some((game, services, window, surface, window_gl, context));
        Ok(())
    }
}

impl FrameTracker {
    pub fn new() -> Self {
        Self {
            rendered_frames: 0,
            last_render: Instant::now(),
            app_start: Instant::now(),
            last_frame_sec: 1.0,
            accum_sec: 0.0,
        }
    }

    fn before_render(&mut self) {
        let now = Instant::now();
        let frame_dur = now - self.last_render;
        self.last_frame_sec = frame_dur.as_secs_f64();
        self.accum_sec = (now - self.app_start).as_secs_f64();
        self.last_render = now;
    }

    fn last_frame_time(&mut self) -> f64 {
        self.last_frame_sec
    }

    fn accumulated_time(&mut self) -> f64 {
        self.accum_sec
    }

    fn number_of_renders(&mut self) -> u64 {
        self.rendered_frames
    }

    fn after_render(&mut self) {
        self.rendered_frames += 1;
    }
}

impl winit::application::ApplicationHandler<UscInputEvent> for UscApp {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.init(event_loop)
            .expect("Failed to initialize game state");
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let Some((game, services, window, surface, window_gl, context)) = self.state.as_mut()
        else {
            return;
        };
        let g = &mut self.frame_tracker;

        if let WindowEvent::RedrawRequested = event {
            self.update_tracker.set();
            for _ in &mut self.update_tracker {
                game.update();
            }
            g.before_render();
            let frame_input = FrameInput {
                events: vec![],
                elapsed_time: g.last_frame_time() * 1000.0,
                accumulated_time: g.accumulated_time() * 1000.0,
                viewport: Viewport {
                    x: 0,
                    y: 0,
                    width: window.inner_size().width,
                    height: window.inner_size().height,
                },
                window_width: window.outer_size().width,
                window_height: window.outer_size().height,
                device_pixel_ratio: 1.0,
                first_frame: g.number_of_renders() == 0,
                context: context.clone(),
            };
            g.after_render();

            let frame_out = game.render(frame_input, window, surface, window_gl);
            surface.swap_buffers(window_gl);
            window.request_redraw();
            if frame_out.exit {
                event_loop.exit();
            }
        } else {
            if let WindowEvent::Resized(_) = event {
                window.resize_surface(surface, window_gl);
            }

            game.handle(
                window,
                &winit::event::Event::WindowEvent { window_id, event },
            );
        }
    }

    fn user_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        event: UscInputEvent,
    ) {
        let Some((game, services, window, surface, window_gl, context)) = self.state.as_mut()
        else {
            return;
        };

        game.handle(window, &winit::event::Event::UserEvent(event));
    }

    fn device_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        device_id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        let Some((game, services, window, surface, window_gl, context)) = self.state.as_mut()
        else {
            return;
        };

        game.handle(
            window,
            &winit::event::Event::DeviceEvent { device_id, event },
        );
    }
}

fn get_log_config(level: log::LevelFilter) -> log4rs::Config {
    use log4rs::append::file::FileAppender;
    use log4rs::config::*;
    use log4rs::encode::pattern::PatternEncoder;
    let encoder = PatternEncoder::new("[{d(%Y-%m-%d %H:%M:%S)}] [{h({l})}] [{t}] {m}{n}");
    let stdout = log4rs::append::console::ConsoleAppender::builder()
        .encoder(Box::new(encoder.clone()))
        .build();

    let mut log_path = default_game_dir();
    log_path.push("game.log");
    let file = FileAppender::builder()
        .append(false)
        .encoder(Box::new(encoder))
        .build(log_path)
        .expect("Failed to create file logger");

    log4rs::Config::builder()
        .appender(Appender::builder().build("stdout", Box::new(stdout)))
        .appender(Appender::builder().build("file", Box::new(file)))
        .build(
            log4rs::config::Root::builder()
                .appender("file")
                .appender("stdout")
                .build(level),
        )
        .expect("Failed to build log config")
}

fn main() -> anyhow::Result<()> {
    let eventloop = winit::event_loop::EventLoop::<UscInputEvent>::with_user_event().build()?;

    let _logger_handle =
        log4rs::init_config(get_log_config(LevelFilter::Info)).expect("Failed to get logger");
    let mut config_path = default_game_dir();
    config_path.push("Main.cfg");
    let args = Args::parse();

    if let Some(mut p) = args.companion_schema {
        for (path, contents) in companion_interface::print_schema() {
            p.push(path);
            _ = std::fs::write(&p, contents);
            p.pop();
        }

        p.push("types.ts");
        companion_interface::print_ts(p.to_str().unwrap());
        return Ok(());
    }

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

    {
        sink.append(mixer);
        sink.play();
        sink.set_volume(GameConfig::get().master_volume);
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let _tokio = rt.enter();

    let mut input = gilrs::GilrsBuilder::default()
        .add_included_mappings(false)
        .with_default_filters(false)
        .add_mappings(&GameConfig::get().mappings.join("\n"))
        .build()
        .expect("Failed to create input context");

    while input.next_event().is_some() {} //empty events

    input
        .gamepads()
        .for_each(|(_, g)| info!("{} uuid: {}", g.name(), uuid::Uuid::from_bytes(g.uuid())));
    let input = Arc::new(Mutex::new(input));
    let gilrs_state = input.clone();
    let companion_service = RwLock::new(companion_interface::CompanionServer::new(
        eventloop.create_proxy(),
    ));

    let _mousex = 0.0;
    let _mousey = 0.0;

    let event_proxy = eventloop.create_proxy();
    let (mut rusc_filter, offset_tx) = RuscFilter::new(GameConfig::get().global_offset as _);

    let _input_thread = poll_promise::Promise::spawn_thread("gilrs", move || {
        let mut knob_state = LaserState::default();
        let binding_filter = CustomBindingFilter;
        loop {
            rusc_filter.update();
            use button_codes::*;
            use gilrs::*;
            use winit::event::ElementState::*;
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

    // Export luals definitions
    export_luals_defs()?;

    let _frame_times = [16.0; FRAME_ACC_SIZE];
    let _frame_time_index = 0;

    eventloop.run_app(&mut UscApp {
        state: None,
        gilrs: gilrs_state,
        mixer_controls,
        sink: Some(sink),
        companion_service: Some(companion_service),
        offset_tx,
        frame_tracker: FrameTracker::new(),
        update_tracker: UpdateTracker::new(),
    });

    Ok(())
}

fn export_luals_defs() -> Result<(), anyhow::Error> {
    use std::io::Write;
    let mut path = default_game_dir();
    path.push("luals");
    std::fs::create_dir_all(&path)?;
    path.push("gfx.lua");
    let mut f = std::fs::File::create(&path)?;
    writeln!(f, "---@meta")?;
    // TODO:
    // luals_gen_tealr::Generator::write_type::<crate::Vgfx>("gfx", f)?;

    path.set_file_name("game.lua");
    let mut f = std::fs::File::create(&path)?;
    writeln!(f, "---@meta")?;
    // TODO:
    // luals_gen_tealr::Generator::write_type::<crate::game_data::GameData>("game", f)?;

    path.set_file_name("shadedmesh.lua");
    let mut f = std::fs::File::create(&path)?;
    writeln!(f, "---@meta")?;
    // TODO:
    // luals_gen_tealr::Generator::write_type::<crate::shaded_mesh::ShadedMesh>("ShadedMesh", f)?;

    path.set_file_name("result.lua");
    let mut f = std::fs::File::create(&path)?;
    writeln!(f, "---@meta")?;
    LuaLsGen::generate_types::<results::SongResultData>(&mut f)?;
    writeln!(f)?;
    writeln!(f, "---@type SongResultData")?;
    writeln!(f, "result = {{}}")?;

    path.set_file_name("gameplay.lua");
    let mut f = std::fs::File::create(&path)?;
    writeln!(f, "---@meta")?;
    LuaLsGen::generate_types::<game::LuaGameState>(&mut f)?;
    writeln!(f)?;
    writeln!(f, "---@type LuaGameState")?;
    writeln!(f, "gameplay = {{}}")?;

    Ok(())
}

#[macro_export]
macro_rules! log_result {
    ($expression:expr) => {{
        let _result_ = $expression;
        if let Err(e) = &_result_ {
            log::warn!("{e}");
        }
    }};
}

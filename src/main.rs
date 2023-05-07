use std::{
    fs::File,
    io::Write,
    path::Path,
    rc::Rc,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use crate::{
    button_codes::LaserState, config::GameConfig, game_main::GameMain,
    skin_settings::SkinSettingEntry, transition::Transition, vg_ui::Vgfx,
};
use directories::ProjectDirs;
use femtovg as vg;

use game_main::ControlMessage;
use generational_arena::{Arena, Index};
use gilrs::ev::filter::Jitter;

use log::*;

use scene::Scene;

use td::{FrameInput, HasContext, Viewport};
use tealr::mlu::mlua::Lua;
use three_d as td;

use glutin::prelude::*;

mod animation;
mod audio;
mod button_codes;
mod config;
mod game;
mod game_data;
mod game_main;
mod help;
mod main_menu;
mod material;
mod results;
mod scene;
mod shaded_mesh;
mod skin_settings;
mod song_provider;
mod songselect;
mod transition;
mod util;
mod vg_ui;
mod window;

pub fn project_dirs() -> ProjectDirs {
    directories::ProjectDirs::from("", "Drewol", "USC").expect("Failed to get project dirs")
}

#[derive(Default)]
pub struct Scenes {
    pub active: Vec<Box<dyn Scene>>,
    pub loaded: Vec<Box<dyn Scene>>,
    pub initialized: Vec<Box<dyn Scene>>,
    pub transition: Option<Transition>,
    should_outro: bool,
}

impl Scenes {
    pub fn tick(
        &mut self,
        dt: f64,
        knob_state: crate::button_codes::LaserState,
        load_lua: Rc<dyn Fn(Rc<Lua>, &'static str) -> anyhow::Result<Index>>,
        app_control_tx: std::sync::mpsc::Sender<ControlMessage>,
    ) {
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
            let result = x.init(load_lua.clone(), app_control_tx.clone());
            if let Err(e) = &result {
                log::error!("{:?}", e);
            }
            result.is_ok()
        });

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
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
            && self.loaded.is_empty()
            && self.initialized.is_empty()
            && self.transition.is_none()
    }

    pub fn render(&mut self, frame: FrameInput<()>, _vgfx: &Arc<Mutex<Vgfx>>) {
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
                log::error!("{:?}", e)
            };
        }

        if let Some(transition) = self.transition.as_mut() {
            transition.render(dt, td_context, &mut target, viewport);
            if let Err(e) = transition.render_ui(dt) {
                log::error!("{:?}", e)
            };
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
    puffin::set_scopes_on(true);
    let mut config_path = std::env::current_dir().unwrap();
    config_path.push("Main.cfg");
    GameConfig::init(config_path);

    let show_debug_ui = false;

    let (window, surface, canvas, gl_context, eventloop, window_gl) = window::create_window();

    let gl_context = Arc::new(gl_context);
    let canvas = Arc::new(Mutex::new(canvas));

    let skin_setting: Vec<SkinSettingEntry> = {
        let skin = &GameConfig::get().unwrap().skin;
        let mut config_def_path = std::env::current_dir()?;
        config_def_path.push("skins");
        config_def_path.push(skin);
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
        .add_included_mappings(true)
        .with_default_filters(false)
        .add_mappings("03000000d01600006d0a000000000000,Pocket Voltex Rev4,a:b1,b:b2,y:b3,x:b4,leftshoulder:b5,rightshoulder:b6,start:b0,leftx:a0,rightx:a1")
        .build()
        .expect("Failed to create input context");

    while input.next_event().is_some() {} //empty events
    let context = td::Context::from_gl_context(gl_context.clone())?;

    let vgfx = Arc::new(Mutex::new(vg_ui::Vgfx::new(
        canvas.clone(),
        std::env::current_dir()?,
    )));

    let mousex = 0.0;
    let mousey = 0.0;

    let event_proxy = eventloop.create_proxy();

    let _input_thread = poll_promise::Promise::spawn_thread("gilrs", move || {
        input
            .gamepads()
            .for_each(|(_, g)| info!("{} uuid: {}", g.name(), uuid::Uuid::from_bytes(g.uuid())));
        let mut knob_state = LaserState::default();

        loop {
            use button_codes::*;
            use game_loop::winit::event::ElementState::*;
            use gilrs::*;
            let e = input.next_event();
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
                    info!("Gilrs thread closing: {:?}", send_err);
                    return;
                }
            } else {
                std::thread::sleep(Duration::from_millis(1))
            }
        }
    });

    let typedef_folder = Path::new("types");
    if !typedef_folder.exists() {
        std::fs::create_dir_all(typedef_folder)?;
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

    let mut typedef_file_path = typedef_folder.to_path_buf();
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

    let mut scenes = Scenes::default();

    scenes.loaded.push(Box::new(main_menu::MainMenu::new()));
    let game_data = Arc::new(Mutex::new(game_data::GameData {
        mouse_pos: (mousex, mousey),
        resolution: (800, 600),
        profile_stack: vec![],
        laser_state: LaserState::default(),
    }));

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
    );

    game_loop::game_loop(
        eventloop,
        window,
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
        move |g, e| g.game.handle(e),
    );
}

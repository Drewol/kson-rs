use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex, MutexGuard, RwLock},
};

use crate::{
    button_codes::LaserState,
    config::GameConfig,
    game_data::{ExportGame, GameData},
    skin_settings::SkinSettingEntry,
    transition::Transition,
    vg_ui::{ExportVgfx, Vgfx},
};
use directories::ProjectDirs;
use femtovg as vg;
use game::HitWindow;
use generational_arena::{Arena, Index};
use gilrs::ev::filter::Jitter;
use kson::Chart;
use log::*;
use main_menu::MainMenuButton;
use puffin::{profile_function, profile_scope};
use scene::Scene;

use td::{FrameInput, FrameOutput, HasContext, SurfaceSettings, Viewport};
use tealr::mlu::mlua::{Lua, LuaSerdeExt};
use three_d as td;
use ureq::json;

mod animation;
mod audio;
mod button_codes;
mod config;
mod game;
mod game_data;
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

    pub fn render(&mut self, frame: FrameInput<()>, vgfx: &Arc<Mutex<Vgfx>>) {
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

    pub fn clear(&mut self) {
        self.active.clear();
        self.initialized.clear();
        self.loaded.clear();
        self.transition = None;
    }
}

pub enum ControlMessage {
    None,
    MainMenu(MainMenuButton),
    Song {
        song: Arc<songselect::Song>,
        diff: usize,
        loader: Box<dyn FnOnce() -> (Chart, Box<dyn rodio::Source<Item = i16>>) + Send>,
    },
    TransitionComplete(Box<dyn scene::Scene>),
    Result {
        song: Arc<songselect::Song>,
        diff_idx: usize,
        score: u32,
        gauge: f32,
    },
}

impl Default for ControlMessage {
    fn default() -> Self {
        Self::None
    }
}

fn main() -> anyhow::Result<()> {
    simple_logger::init_with_level(Level::Info)?;
    puffin::set_scopes_on(true);

    let window = td::Window::new(td::WindowSettings {
        title: "Test".to_string(),
        max_size: Some((1280, 720)),
        surface_settings: SurfaceSettings {
            multisamples: 4,
            stencil_buffer: 8,
            vsync: true,
            depth_buffer: 8,
            hardware_acceleration: td::HardwareAcceleration::Required,
        },
        ..Default::default()
    })
    .unwrap();

    let mut config_path = std::env::current_dir().unwrap();
    config_path.push("Main.cfg");

    GameConfig::init(config_path);

    let skin_setting: Vec<SkinSettingEntry> = {
        let skin = &GameConfig::get().unwrap().skin;
        let mut config_def_path = std::env::current_dir()?;
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
        .add_included_mappings(true)
        .with_default_filters(false)
        .add_mappings("03000000d01600006d0a000000000000,Pocket Voltex Rev4,a:b1,b:b2,y:b3,x:b4,leftshoulder:b5,rightshoulder:b6,start:b0,leftx:a0,rightx:a1")
        .build()
        .expect("Failed to create input context");

    while input.next_event().is_some() {} //empty events

    let context = window.gl();
    let renderer = unsafe {
        vg::renderer::OpenGl::new_from_context(
            std::mem::transmute_copy(&**context),
            context.version().is_embedded,
        )
        .expect("awd")
    };

    let canvas = Arc::new(Mutex::new(
        vg::Canvas::new(renderer).expect("Failed to create canvas"),
    ));
    let vgfx = Arc::new(Mutex::new(vg_ui::Vgfx::new(
        canvas.clone(),
        std::env::current_dir()?,
    )));

    // Create a CPU-side mesh consisting of a single colored triangle
    let positions = vec![
        td::vec3(0.5, -0.5, 0.0),  // bottom right
        td::vec3(-0.5, -0.5, 0.0), // bottom left
        td::vec3(0.0, 0.5, 0.0),   // top
    ];
    let colors = vec![
        td::Color::new(255, 0, 0, 255), // bottom right
        td::Color::new(0, 255, 0, 255), // bottom left
        td::Color::new(0, 0, 255, 255), // top
    ];
    let cpu_mesh = td::CpuMesh {
        positions: td::Positions::F32(positions),
        colors: Some(colors),
        ..Default::default()
    };

    // Construct a model, with a default color material, thereby transferring the mesh data to the GPU
    let mut model = td::Gm::new(
        td::Mesh::new(&context, &cpu_mesh),
        td::ColorMaterial::default(),
    );

    let mut camera = td::Camera::new_perspective(
        window.viewport(),
        td::vec3(0.0, 0.0, 2.0),
        td::vec3(0.0, 0.0, 0.0),
        td::vec3(0.0, 1.0, 0.0),
        td::degrees(45.0),
        0.1,
        10.0,
    );

    let mut mousex = 0.0;
    let mut mousey = 0.0;

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
    let mut gui = three_d::GUI::new(&context);

    const FRAME_ACC_SIZE: usize = 16;
    let mut frame_times = [16.0; FRAME_ACC_SIZE];
    let mut frame_time_index = 0;

    let fps_paint = vg::Paint::color(vg::Color::white()).with_text_align(vg::Align::Right);

    let mut scenes = Scenes::default();

    scenes.loaded.push(Box::new(main_menu::MainMenu::new()));
    let game_data = Arc::new(Mutex::new(game_data::GameData {
        mouse_pos: (mousex, mousey),
        resolution: (800, 600),
        profile_stack: vec![],
    }));

    let lua_arena: Rc<RwLock<Arena<Rc<Lua>>>> = Rc::new(RwLock::new(Arena::new()));

    let mut transition_lua_idx = Index::from_raw_parts(0, 0);
    let mut transition_song_lua_idx = Index::from_raw_parts(0, 0);

    let jitter_filter = Jitter { threshold: 0.005 };
    let mut knob_state = LaserState::default();

    let (control_tx, control_rx) = std::sync::mpsc::channel();
    let mut second_frame = false;

    window.render_loop(move |mut frame_input| {
        poll_promise::tick(); //Tick async runtime at least once per frame
        knob_state.zero_deltas();
        puffin::profile_scope!("Frame");
        puffin::GlobalProfiler::lock().new_frame();

        for (idx, lua) in lua_arena.read().unwrap().iter() {
            lua.set_app_data(frame_input.clone());
        }
        let lua_frame_input = frame_input.clone();

        let load_lua = |game_data: Arc<Mutex<GameData>>,
                        vgfx: Arc<Mutex<Vgfx>>,
                        arena: Rc<RwLock<Arena<Rc<Lua>>>>| {
            let lua_frame_input = lua_frame_input.clone();
            Rc::new(move |lua: Rc<Lua>, script_path| {
                //Set path for 'require' (https://stackoverflow.com/questions/4125971/setting-the-global-lua-path-variable-from-c-c?lq=1)
                let skin = &GameConfig::get().unwrap().skin;
                let mut real_script_path = std::env::current_dir()?;
                real_script_path.push("skins");
                real_script_path.push(skin);

                tealr::mlu::set_global_env(ExportVgfx, &lua)?;
                tealr::mlu::set_global_env(ExportGame, &lua)?;
                lua.globals()
                    .set(
                        "IRData",
                        lua.to_value(&json!({
                            "Active": false
                        }))
                        .unwrap(),
                    )
                    .unwrap();
                let idx = arena
                    .write()
                    .expect("Could not get lock to lua arena")
                    .insert(lua.clone());
                {
                    lua.set_app_data(vgfx.clone());
                    lua.set_app_data(game_data.clone());
                    lua.set_app_data(idx);
                    lua.set_app_data(lua_frame_input.clone());
                    lua.gc_stop();
                }

                {
                    let package: tealr::mlu::mlua::Table = lua.globals().get("package").unwrap();
                    let package_path: String = package.get("path").unwrap();
                    let package_path = format!(
                        "{};{}/scripts/?.lua;{}/scripts/?",
                        package_path,
                        real_script_path.as_os_str().to_string_lossy(),
                        real_script_path.as_os_str().to_string_lossy()
                    );
                    package.set("path", package_path).unwrap();

                    lua.globals().set("package", package).unwrap();
                }

                real_script_path.push("scripts");

                real_script_path.push("common.lua");
                if real_script_path.exists() {
                    info!("Loading: {:?}", &real_script_path);
                    let test_code = std::fs::read_to_string(&real_script_path)?;
                    lua.load(&test_code).set_name("common.lua")?.eval::<()>()?;
                }

                real_script_path.pop();

                real_script_path.push(script_path);
                info!("Loading: {:?}", &real_script_path);
                let test_code = std::fs::read_to_string(real_script_path)?;
                {
                    profile_scope!("evaluate lua file");
                    lua.load(&test_code).set_name(script_path)?.eval::<()>()?;
                }
                Ok(idx)
            })
        };

        if frame_input.first_frame {
            frame_input
                .screen()
                .clear(td::ClearState::color(0.0, 0.0, 0.0, 1.0));
            let mut canvas = canvas.lock().unwrap();
            canvas.reset();
            canvas.set_size(frame_input.viewport.width, frame_input.viewport.height, 1.0);
            canvas.fill_text(
                10.0,
                10.0,
                "Loading...",
                &vg::Paint::color(vg::Color::white())
                    .with_font_size(32.0)
                    .with_text_baseline(vg::Baseline::Top),
            );
            canvas.flush();
            second_frame = true;

            return FrameOutput {
                swap_buffers: true,
                wait_next_event: false,
                ..Default::default()
            };
        }
        if second_frame {
            let transition_lua = Rc::new(Lua::new());
            let loader_fn = load_lua(game_data.clone(), vgfx.clone(), lua_arena.clone());
            transition_lua_idx = loader_fn(transition_lua, "transition.lua").unwrap();

            let transition_song_lua = Rc::new(Lua::new());
            transition_song_lua_idx = loader_fn(transition_song_lua, "songtransition.lua").unwrap();
            second_frame = false;
        }

        //Initialize loaded scenes
        scenes.tick(
            frame_input.elapsed_time,
            knob_state,
            load_lua(game_data.clone(), vgfx.clone(), lua_arena.clone()),
            control_tx.clone(),
        );

        while let Ok(control_msg) = control_rx.try_recv() {
            match control_msg {
                ControlMessage::None => {}
                ControlMessage::MainMenu(b) => match b {
                    MainMenuButton::Start => {
                        scenes.suspend_top();

                        if let Ok(arena) = lua_arena.read() {
                            let transition_lua = arena.get(transition_lua_idx).unwrap().clone();
                            scenes.transition = Some(Transition::new(
                                transition_lua,
                                ControlMessage::MainMenu(MainMenuButton::Start),
                                control_tx.clone(),
                                frame_input.context.clone(),
                                vgfx.clone(),
                                frame_input.viewport,
                            ))
                        }
                    }
                    MainMenuButton::Downloads => {}
                    MainMenuButton::Exit => {
                        scenes.clear();
                    }
                    _ => {}
                },
                ControlMessage::Song { diff, loader, song } => {
                    if let Ok(arena) = lua_arena.read() {
                        let transition_lua = arena.get(transition_song_lua_idx).unwrap().clone();
                        scenes.transition = Some(Transition::new(
                            transition_lua,
                            ControlMessage::Song { diff, loader, song },
                            control_tx.clone(),
                            frame_input.context.clone(),
                            vgfx.clone(),
                            frame_input.viewport,
                        ))
                    }
                }
                ControlMessage::TransitionComplete(mut scene_data) => {
                    scenes.loaded.push(scene_data)
                }
                ControlMessage::Result {
                    song,
                    diff_idx,
                    score,
                    gauge,
                } => {
                    if let Ok(arena) = lua_arena.read() {
                        let transition_lua = arena.get(transition_lua_idx).unwrap().clone();
                        scenes.transition = Some(Transition::new(
                            transition_lua,
                            ControlMessage::Result {
                                song,
                                diff_idx,
                                score,
                                gauge,
                            },
                            control_tx.clone(),
                            frame_input.context.clone(),
                            vgfx.clone(),
                            frame_input.viewport,
                        ))
                    }
                }
            }
        }

        camera.set_viewport(frame_input.viewport);
        // Set the current transformation of the triangle
        model.set_transformation(td::Mat4::from_angle_y(td::radians(
            (frame_input.accumulated_time * 0.005) as f32,
        )));

        frame_times[frame_time_index as usize] = frame_input.elapsed_time;
        frame_time_index = (frame_time_index + 1) % FRAME_ACC_SIZE;
        let fps = 1000_f64 / (frame_times.iter().sum::<f64>() / FRAME_ACC_SIZE as f64);

        for event in &mut frame_input.events {
            if let td::Event::MouseMotion {
                button: _,
                delta: _,
                position,
                modifiers: _,
                handled: _,
            } = *event
            {
                (mousex, mousey) = position;
            }

            for scene in scenes.active.iter_mut().filter(|s| !s.is_suspended()) {
                scene.on_event(event); //TODO: break on event handled
            }
        }

        while let Some(e) = input.next_event() {
            match e.event {
                gilrs::EventType::ButtonPressed(button, _) => {
                    let button = button_codes::UscButton::from(button);
                    info!("{:?}", button);
                    scenes
                        .active
                        .iter_mut()
                        .filter(|s| !s.is_suspended())
                        .for_each(|s| s.on_button_pressed(button))
                }
                gilrs::EventType::ButtonRepeated(_, _) => {}
                gilrs::EventType::ButtonReleased(_, _) => {}
                gilrs::EventType::ButtonChanged(_, _, _) => {}
                gilrs::EventType::AxisChanged(axis, value, _) => match axis {
                    gilrs::Axis::LeftStickX => knob_state.update(kson::Side::Left, value),
                    gilrs::Axis::RightStickX => knob_state.update(kson::Side::Right, value),
                    e => {
                        info!("{:?}", e)
                    }
                },
                gilrs::EventType::Connected => {}
                gilrs::EventType::Disconnected => {}
                gilrs::EventType::Dropped => {}
            }
        }

        if frame_input.first_frame {
            input.gamepads().for_each(|(_, g)| {
                info!("{} uuid: {}", g.name(), uuid::Uuid::from_bytes(g.uuid()))
            });
        }

        update_game_data_and_clear(&game_data, mousex, mousey, &frame_input);

        reset_viewport_size(&vgfx, &frame_input);

        scenes.render(frame_input.clone(), &vgfx);
        render_overlays(&vgfx, &frame_input, fps, &fps_paint);

        debug_ui(&mut gui, frame_input, &mut scenes);

        run_lua_gc(&lua_arena);

        game_data.lock().map(|mut a| a.profile_stack.clear());

        let exit = scenes.is_empty();
        if exit {
            if let Some(c) = GameConfig::get() {
                c.save()
            }
        }

        td::FrameOutput {
            exit,
            swap_buffers: true,
            wait_next_event: false,
        }
    });

    Ok(())
}

fn run_lua_gc(lua_arena: &Rc<RwLock<Arena<Rc<Lua>>>>) {
    profile_scope!("Garbage collect");
    for (idx, lua) in lua_arena.read().unwrap().iter() {
        //TODO: if reference count = 1, remove loaded gfx assets for state
        lua.gc_collect();
        lua.gc_collect();
    }
}

fn debug_ui(gui: &mut td::GUI, mut frame_input: td::FrameInput<()>, scenes: &mut Scenes) {
    profile_function!();
    gui.update(
        &mut frame_input.events,
        frame_input.accumulated_time,
        frame_input.viewport,
        frame_input.device_pixel_ratio,
        |gui_context| {
            if let Some(s) = scenes.active.last_mut() {
                s.debug_ui(gui_context);
            }
            puffin_egui::profiler_window(gui_context);
            three_d::egui::Window::new("Scenes").show(gui_context, |ui| {
                ui.label("Loaded");
                for ele in &scenes.loaded {
                    ui.label(ele.name());
                }
                ui.separator();
                ui.label("Initialized");
                for ele in &scenes.initialized {
                    ui.label(ele.name());
                }
                ui.separator();
                ui.label("Active");

                let mut closed_scene = None;

                for (i, ele) in scenes.active.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(ele.name());
                        if ui.button("Close").clicked() {
                            closed_scene = Some(i);
                        }
                    });
                }

                if let Some(closed) = closed_scene {
                    scenes.active.remove(closed);
                }

                if scenes.transition.is_some() {
                    ui.label("Transitioning");
                }
            });
        },
    );
    frame_input.screen().write(|| gui.render());
}

fn render_overlays(
    vgfx: &Arc<Mutex<Vgfx>>,
    frame_input: &td::FrameInput<()>,
    fps: f64,
    fps_paint: &vg::Paint,
) {
    let vgfx_lock = vgfx.try_lock();
    if let Ok(vgfx) = vgfx_lock {
        let mut canvas_lock = vgfx.canvas.try_lock();
        if let Ok(ref mut canvas) = canvas_lock {
            canvas.reset();
            canvas.set_size(frame_input.viewport.width, frame_input.viewport.height, 1.0);
            canvas.fill_text(
                frame_input.viewport.width as f32 - 5.0,
                frame_input.viewport.height as f32 - 5.0,
                format!("{:.1} FPS", fps),
                &fps_paint,
            );
            canvas.flush();
        }
    }
}

fn update_game_data_and_clear(
    game_data: &Arc<Mutex<GameData>>,
    mousex: f64,
    mousey: f64,
    frame_input: &td::FrameInput<()>,
) {
    {
        let lock = game_data.lock();
        if let Ok(mut game_data) = lock {
            *game_data = GameData {
                mouse_pos: (mousex, mousey),
                resolution: (frame_input.viewport.width, frame_input.viewport.height),
                profile_stack: std::mem::take(&mut game_data.profile_stack),
            };
        }
    }

    {
        frame_input
            .screen()
            .clear(td::ClearState::color_and_depth(0.0, 0.0, 0.0, 0.0, 1.0));
        // .render(&camera, [&model], &[]);
    }
}

fn reset_viewport_size(vgfx: &Arc<Mutex<Vgfx>>, frame_input: &td::FrameInput<()>) {
    let vgfx_lock = vgfx.try_lock();
    if let Ok(vgfx) = vgfx_lock {
        let mut canvas_lock = vgfx.canvas.try_lock();
        if let Ok(ref mut canvas) = canvas_lock {
            canvas.reset();
            canvas.set_size(frame_input.viewport.width, frame_input.viewport.height, 1.0);
            canvas.flush();
        }
    }
}

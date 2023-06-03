use std::{
    rc::Rc,
    sync::{
        mpsc::{Receiver, Sender},
        Arc, Mutex, RwLock,
    },
};

use egui_glow::EguiGlow;
use femtovg::Paint;
use generational_arena::{Arena, Index};

use kson::Chart;
use log::*;
use puffin::{profile_function, profile_scope};
use serde_json::json;
use td::FrameOutput;
use tealr::mlu::mlua::Lua;
use three_d::FrameInput;

use femtovg as vg;
use three_d as td;
use vg::{renderer::OpenGl, Canvas};

use tealr::mlu::mlua::LuaSerdeExt;

use crate::{
    button_codes::{LaserState, UscInputEvent},
    config::GameConfig,
    game_data::{ExportGame, GameData},
    input_state::InputState,
    main_menu::MainMenuButton,
    scene, songselect,
    transition::Transition,
    vg_ui::{ExportVgfx, Vgfx},
    Scenes, FRAME_ACC_SIZE,
};

type SceneLoader = dyn FnOnce() -> (Chart, Box<dyn rodio::Source<Item = f32> + Send>) + Send;

pub enum ControlMessage {
    None,
    MainMenu(MainMenuButton),
    Song {
        song: Arc<songselect::Song>,
        diff: usize,
        loader: Box<SceneLoader>,
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

pub struct GameMain {
    lua_arena: Rc<RwLock<Arena<Rc<Lua>>>>,
    scenes: Scenes,
    control_tx: Sender<ControlMessage>,
    control_rx: Receiver<ControlMessage>,
    knob_state: LaserState,
    frame_times: [f64; 16],
    frame_time_index: usize,
    fps_paint: Paint,
    transition_lua_idx: Index,
    transition_song_lua_idx: Index,
    game_data: Arc<Mutex<GameData>>,
    vgfx: Arc<Mutex<Vgfx>>,
    canvas: Arc<Mutex<Canvas<OpenGl>>>,
    frame_count: u32,
    gui: EguiGlow,
    show_debug_ui: bool,
    mousex: f64,
    mousey: f64,
    input_state: Arc<InputState>,
}

impl GameMain {
    pub fn new(
        lua_arena: Rc<RwLock<Arena<Rc<Lua>>>>,
        scenes: Scenes,
        control_tx: Sender<ControlMessage>,
        control_rx: Receiver<ControlMessage>,
        knob_state: LaserState,
        frame_times: [f64; 16],
        frame_time_index: usize,
        fps_paint: Paint,
        transition_lua_idx: Index,
        transition_song_lua_idx: Index,
        game_data: Arc<Mutex<GameData>>,
        vgfx: Arc<Mutex<Vgfx>>,
        canvas: Arc<Mutex<Canvas<OpenGl>>>,
        frame_count: u32,
        gui: EguiGlow,
        show_debug_ui: bool,
        mousex: f64,
        mousey: f64,
        input_state: Arc<InputState>,
    ) -> Self {
        Self {
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
            frame_count,
            gui,
            show_debug_ui,
            mousex,
            mousey,
            input_state,
        }
    }

    pub fn update(&mut self) {}
    pub fn render(
        &mut self,
        frame_input: FrameInput<()>,
        window: &game_loop::winit::window::Window,
    ) -> FrameOutput {
        let GameMain {
            lua_arena,
            scenes,
            control_tx,
            control_rx,
            knob_state,
            frame_times,
            fps_paint,
            transition_lua_idx,
            transition_song_lua_idx,
            canvas,
            frame_count,
            game_data,
            vgfx,
            show_debug_ui,
            gui,
            frame_time_index,
            mousex,
            mousey,
            input_state,
        } = self;

        poll_promise::tick(); //Tick async runtime at least once per frame
        knob_state.zero_deltas();
        puffin::profile_scope!("Frame");
        puffin::GlobalProfiler::lock().new_frame();

        for (_idx, lua) in lua_arena.read().unwrap().iter() {
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
                    vgfx.lock().unwrap().init_asset_scope(idx)
                }

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
            *frame_count += 1;

            return FrameOutput {
                swap_buffers: true,
                wait_next_event: false,
                ..Default::default()
            };
        }
        if *frame_count == 1 {
            let transition_lua = Rc::new(Lua::new());
            let loader_fn = load_lua(game_data.clone(), vgfx.clone(), lua_arena.clone());
            *transition_lua_idx = loader_fn(transition_lua, "transition.lua").unwrap();

            let transition_song_lua = Rc::new(Lua::new());
            *transition_song_lua_idx =
                loader_fn(transition_song_lua, "songtransition.lua").unwrap();
            *frame_count += 1;
        }

        //Initialize loaded scenes
        scenes.tick(
            frame_input.elapsed_time,
            *knob_state,
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
                            let transition_lua = arena.get(*transition_lua_idx).unwrap().clone();
                            scenes.transition = Some(Transition::new(
                                transition_lua,
                                ControlMessage::MainMenu(MainMenuButton::Start),
                                control_tx.clone(),
                                frame_input.context.clone(),
                                vgfx.clone(),
                                frame_input.viewport,
                                self.input_state.clone(),
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
                        let transition_lua = arena.get(*transition_song_lua_idx).unwrap().clone();
                        scenes.transition = Some(Transition::new(
                            transition_lua,
                            ControlMessage::Song { diff, loader, song },
                            control_tx.clone(),
                            frame_input.context.clone(),
                            vgfx.clone(),
                            frame_input.viewport,
                            self.input_state.clone(),
                        ))
                    }
                }
                ControlMessage::TransitionComplete(scene_data) => scenes.loaded.push(scene_data),
                ControlMessage::Result {
                    song,
                    diff_idx,
                    score,
                    gauge,
                } => {
                    if let Ok(arena) = lua_arena.read() {
                        let transition_lua = arena.get(*transition_lua_idx).unwrap().clone();
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
                            self.input_state.clone(),
                        ))
                    }
                }
            }
        }

        frame_times[*frame_time_index] = frame_input.elapsed_time;
        *frame_time_index = (*frame_time_index + 1) % FRAME_ACC_SIZE;
        let fps = 1000_f64 / (frame_times.iter().sum::<f64>() / FRAME_ACC_SIZE as f64);

        Self::update_game_data_and_clear(game_data, *mousex, *mousey, &frame_input, *knob_state);

        Self::reset_viewport_size(vgfx.clone(), &frame_input);

        scenes.render(frame_input.clone(), vgfx);
        Self::render_overlays(vgfx, &frame_input, fps, fps_paint);

        if *show_debug_ui {
            Self::debug_ui(gui, window, scenes);
        }

        Self::run_lua_gc(
            lua_arena,
            &mut vgfx.lock().unwrap(),
            *transition_lua_idx,
            *transition_song_lua_idx,
        );

        if let Ok(mut a) = game_data.lock() {
            a.profile_stack.clear()
        }

        let exit = scenes.is_empty();
        if exit {
            if let Some(c) = GameConfig::get() {
                c.save();
            }
        }

        FrameOutput {
            exit,
            swap_buffers: true,
            wait_next_event: false,
        }
    }
    pub fn handle(&mut self, event: &game_loop::winit::event::Event<UscInputEvent>) {
        use game_loop::winit::event::*;
        if let Event::WindowEvent {
            window_id: _,
            event,
        } = event
        {
            let event_response = self.gui.on_event(event);
            if event_response.consumed {
                return;
            }
        }

        match event {
            Event::UserEvent(e) => {
                info!("{:?}", e);
                match e {
                    UscInputEvent::Laser(ls) => self.knob_state = *ls,
                    UscInputEvent::Button(b, s) => match s {
                        ElementState::Pressed => {
                            self.scenes.for_each_active_mut(|x| x.on_button_pressed(*b))
                        }
                        ElementState::Released => self
                            .scenes
                            .for_each_active_mut(|x| x.on_button_released(*b)),
                    },
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CursorMoved { position, .. },
                ..
            } => {
                self.mousex = position.x;
                self.mousey = position.y;
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => self.scenes.clear(),
            Event::DeviceEvent {
                event:
                    DeviceEvent::Key(KeyboardInput {
                        virtual_keycode: Some(VirtualKeyCode::D),
                        state: ElementState::Pressed,
                        ..
                    }),
                ..
            } => self.show_debug_ui = !self.show_debug_ui,
            _ => (),
        }

        self.scenes
            .active
            .iter_mut()
            .filter(|x| !x.is_suspended())
            .for_each(|x| x.on_event(event));
    }

    fn run_lua_gc(
        lua_arena: &Rc<RwLock<Arena<Rc<Lua>>>>,
        vgfx: &mut Vgfx,
        transition_lua_idx: Index,
        transition_song_lua_idx: Index,
    ) {
        profile_scope!("Garbage collect");
        lua_arena.write().unwrap().retain(|idx, lua| {
            //TODO: if reference count = 1, remove loaded gfx assets for state
            lua.gc_collect();
            if Rc::strong_count(lua) > 1
                || idx == transition_lua_idx
                || idx == transition_song_lua_idx
            {
                true
            } else {
                vgfx.drop_assets(&idx);
                false
            }
        });
    }

    fn debug_ui(
        gui: &mut EguiGlow,
        window: &game_loop::winit::window::Window,
        scenes: &mut Scenes,
    ) {
        profile_function!();
        gui.run(window, |gui_context| {
            if let Some(s) = scenes.active.last_mut() {
                s.debug_ui(gui_context);
            }
            puffin_egui::profiler_window(gui_context);
            egui::Window::new("Scenes").show(gui_context, |ui| {
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
        });

        gui.paint(window);
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
                    fps_paint,
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
        laser_state: LaserState,
    ) {
        {
            let lock = game_data.lock();
            if let Ok(mut game_data) = lock {
                *game_data = GameData {
                    mouse_pos: (mousex, mousey),
                    resolution: (frame_input.viewport.width, frame_input.viewport.height),
                    profile_stack: std::mem::take(&mut game_data.profile_stack),
                    laser_state,
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

    fn reset_viewport_size(vgfx: Arc<Mutex<Vgfx>>, frame_input: &td::FrameInput<()>) {
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
}

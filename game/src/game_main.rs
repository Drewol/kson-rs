use std::{
    ops::{Add, Sub},
    rc::Rc,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex, RwLock,
    },
    time::{Duration, SystemTime},
};

use di::{RefMut, ServiceProvider};
use egui_glow::EguiGlow;
use femtovg::Paint;
use game_loop::winit::{dpi::PhysicalPosition, event, window::Window};
use generational_arena::{Arena, Index};

use kson::Chart;
use log::*;
use puffin::{profile_function, profile_scope};

use serde_json::json;
use td::{FrameOutput, Modifiers};
use tealr::mlu::mlua::Lua;
use three_d::FrameInput;

use femtovg as vg;
use three_d as td;
use vg::{renderer::OpenGl, Canvas};

use tealr::mlu::mlua::LuaSerdeExt;

use crate::{
    button_codes::{LaserState, UscInputEvent},
    config::GameConfig,
    game::HitRating,
    game_data::{ExportGame, GameData, LuaPath},
    input_state::InputState,
    lua_http::{ExportLuaHttp, LuaHttp},
    lua_service::LuaProvider,
    main_menu::MainMenuButton,
    scene,
    settings_screen::SettingsScreen,
    songselect,
    transition::Transition,
    util::lua_address,
    vg_ui::{ExportVgfx, Vgfx},
    LuaArena, RuscMixer, Scenes, FRAME_ACC_SIZE,
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
        hit_ratings: Vec<HitRating>,
    },
}

impl Default for ControlMessage {
    fn default() -> Self {
        Self::None
    }
}

pub struct GameMain {
    lua_arena: di::RefMut<LuaArena>,
    lua_provider: Arc<LuaProvider>,
    scenes: Scenes,
    control_tx: Sender<ControlMessage>,
    control_rx: Receiver<ControlMessage>,
    knob_state: LaserState,
    frame_times: [f64; 16],
    frame_time_index: usize,
    fps_paint: Paint,
    transition_lua_idx: Index,
    transition_song_lua_idx: Index,
    game_data: Arc<RwLock<GameData>>,
    vgfx: Arc<RwLock<Vgfx>>,
    frame_count: u32,
    gui: EguiGlow,
    show_debug_ui: bool,
    mousex: f64,
    mousey: f64,
    input_state: InputState,
    mixer: RuscMixer,
    modifiers: Modifiers,
    service_provider: ServiceProvider,
}

impl GameMain {
    pub fn new(
        scenes: Scenes,
        fps_paint: Paint,
        gui: EguiGlow,
        show_debug_ui: bool,
        service_provider: ServiceProvider,
    ) -> Self {
        let (control_tx, control_rx) = channel();
        Self {
            lua_arena: service_provider.get_required(),
            lua_provider: service_provider.get_required(),
            scenes,
            control_tx,
            control_rx,
            knob_state: LaserState::default(),
            frame_times: [0.01; 16],
            frame_time_index: 0,
            fps_paint,
            transition_lua_idx: Index::from_raw_parts(0, 0),
            transition_song_lua_idx: Index::from_raw_parts(0, 0),
            game_data: service_provider.get_required_mut(),
            vgfx: service_provider.get_required_mut(),
            frame_count: 0,
            gui,
            show_debug_ui,
            mousex: 0.0,
            mousey: 0.0,
            input_state: InputState::clone(&service_provider.get_required()),
            mixer: service_provider.get_required(),
            modifiers: Modifiers::default(),
            service_provider,
        }
    }

    const KEYBOARD_LASER_SENS: f32 = 1.0 / 240.0;
    pub fn update(&mut self) {
        let should_profile = GameConfig::get().args.profiling;
        if puffin::are_scopes_on() != should_profile {
            puffin::set_scopes_on(should_profile);
        }

        if GameConfig::get().keyboard_knobs {
            let mut ls = LaserState::default();
            for l in [kson::Side::Left, kson::Side::Right] {
                for d in [kson::Side::Left, kson::Side::Right] {
                    if self
                        .input_state
                        .is_button_held(crate::button_codes::UscButton::Laser(l, d))
                        .is_some()
                    {
                        ls.update(
                            l,
                            match d {
                                kson::Side::Left => -Self::KEYBOARD_LASER_SENS,
                                kson::Side::Right => Self::KEYBOARD_LASER_SENS,
                            },
                        )
                    }
                }
            }

            self.scenes.for_each_active_mut(|x| {
                x.on_event(&event::Event::UserEvent(UscInputEvent::Laser(
                    ls,
                    SystemTime::now(),
                )))
            });
        }
    }
    pub fn render(
        &mut self,
        frame_input: FrameInput,
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
            frame_count,
            game_data,
            vgfx,
            show_debug_ui,
            gui,
            frame_time_index,
            mousex,
            mousey,
            input_state: _,
            mixer,
            modifiers: _,
            service_provider,
            lua_provider,
        } = self;

        knob_state.zero_deltas();
        puffin::profile_scope!("Frame");
        puffin::GlobalProfiler::lock().new_frame();

        for (_idx, lua) in lua_arena.read().unwrap().0.iter() {
            lua.set_app_data(frame_input.clone());
        }
        let lua_frame_input = frame_input.clone();
        let lua_mixer = mixer.clone();

        if frame_input.first_frame {
            frame_input
                .screen()
                .clear(td::ClearState::color(0.0, 0.0, 0.0, 1.0));
            let vgfx = vgfx.write().unwrap();
            let mut canvas = vgfx.canvas.lock().unwrap();
            canvas.reset();
            canvas.set_size(frame_input.viewport.width, frame_input.viewport.height, 1.0);
            _ = canvas.fill_text(
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

            *transition_lua_idx = lua_provider
                .register_libraries(transition_lua, "transition.lua")
                .unwrap();

            let transition_song_lua = Rc::new(Lua::new());
            *transition_song_lua_idx = lua_provider
                .register_libraries(transition_song_lua, "songtransition.lua")
                .unwrap();
            *frame_count += 1;
        }

        //Initialize loaded scenes
        scenes.tick(frame_input.elapsed_time, *knob_state, control_tx.clone());

        while let Ok(control_msg) = control_rx.try_recv() {
            match control_msg {
                ControlMessage::None => {}
                ControlMessage::MainMenu(b) => match b {
                    MainMenuButton::Start => {
                        scenes.suspend_top();

                        if let Ok(arena) = lua_arena.read() {
                            let transition_lua = arena.0.get(*transition_lua_idx).unwrap().clone();
                            scenes.transition = Some(Transition::new(
                                transition_lua,
                                ControlMessage::MainMenu(MainMenuButton::Start),
                                control_tx.clone(),
                                frame_input.context.clone(),
                                vgfx.clone(),
                                frame_input.viewport,
                                self.input_state.clone(),
                                game_data.clone(),
                                service_provider.create_scope(),
                            ))
                        }
                    }
                    MainMenuButton::Downloads => {}
                    MainMenuButton::Exit => {
                        scenes.clear();
                    }
                    MainMenuButton::Options => scenes.loaded.push(Box::new(SettingsScreen::new())),
                    _ => {}
                },
                ControlMessage::Song { diff, loader, song } => {
                    if let Ok(arena) = lua_arena.read() {
                        let transition_lua = arena.0.get(*transition_song_lua_idx).unwrap().clone();
                        scenes.transition = Some(Transition::new(
                            transition_lua,
                            ControlMessage::Song { diff, loader, song },
                            control_tx.clone(),
                            frame_input.context.clone(),
                            vgfx.clone(),
                            frame_input.viewport,
                            self.input_state.clone(),
                            game_data.clone(),
                            service_provider.create_scope(),
                        ))
                    }
                }
                ControlMessage::TransitionComplete(scene_data) => scenes.loaded.push(scene_data),
                ControlMessage::Result {
                    song,
                    diff_idx,
                    score,
                    gauge,
                    hit_ratings,
                } => {
                    if let Ok(arena) = lua_arena.read() {
                        let transition_lua = arena.0.get(*transition_lua_idx).unwrap().clone();
                        scenes.transition = Some(Transition::new(
                            transition_lua,
                            ControlMessage::Result {
                                song,
                                diff_idx,
                                score,
                                gauge,
                                hit_ratings,
                            },
                            control_tx.clone(),
                            frame_input.context.clone(),
                            vgfx.clone(),
                            frame_input.viewport,
                            self.input_state.clone(),
                            game_data.clone(),
                            service_provider.create_scope(),
                        ))
                    }
                }
            }
        }

        frame_times[*frame_time_index] = frame_input.elapsed_time;
        *frame_time_index = (*frame_time_index + 1) % FRAME_ACC_SIZE;
        let fps = 1000_f64 / (frame_times.iter().sum::<f64>() / FRAME_ACC_SIZE as f64);

        Self::update_game_data_and_clear(
            game_data,
            *mousex,
            *mousey,
            &frame_input,
            self.input_state.clone(),
        );

        Self::reset_viewport_size(vgfx.clone(), &frame_input);

        scenes.render(frame_input.clone(), vgfx);
        Self::render_overlays(vgfx, &frame_input, fps, fps_paint);

        gui.run(window, |ctx| {
            scenes.render_egui(ctx);

            if *show_debug_ui {
                Self::debug_ui(ctx, scenes);
            }
        });
        gui.paint(window);

        Self::run_lua_gc(
            lua_arena,
            &mut vgfx.write().unwrap(),
            *transition_lua_idx,
            *transition_song_lua_idx,
        );

        if let Ok(mut a) = game_data.write() {
            a.profile_stack.clear()
        }

        let exit = scenes.is_empty();
        if exit {
            GameConfig::get().save()
        }

        FrameOutput {
            exit,
            swap_buffers: true,
            wait_next_event: false,
        }
    }
    pub fn handle(
        &mut self,
        window: &Window,
        event: &game_loop::winit::event::Event<UscInputEvent>,
    ) {
        use game_loop::winit::event::*;
        if let Event::WindowEvent {
            window_id: _,
            event,
        } = event
        {
            if self.show_debug_ui || self.scenes.should_render_egui() {
                let event_response = self.gui.on_event(event);
                if event_response.consumed {
                    return;
                }
            }
        }

        let mut transformed_event = None;

        let (offset, offset_neg) = {
            let global_offset = GameConfig::get().global_offset;
            (
                Duration::from_millis(global_offset.unsigned_abs() as _),
                global_offset < 0,
            )
        };

        match event {
            Event::UserEvent(e) => {
                info!("{:?}", e);
                self.input_state.update(e);
                match e {
                    UscInputEvent::Laser(ls, _time) => self.knob_state = *ls,
                    UscInputEvent::Button(b, s, time) => match s {
                        ElementState::Pressed => self
                            .scenes
                            .for_each_active_mut(|x| x.on_button_pressed(*b, *time)),
                        ElementState::Released => self
                            .scenes
                            .for_each_active_mut(|x| x.on_button_released(*b, *time)),
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
                event: WindowEvent::ModifiersChanged(mods),
                ..
            } => {
                self.modifiers = Modifiers {
                    alt: mods.alt(),
                    ctrl: mods.ctrl(),
                    shift: mods.shift(),
                    command: mods.ctrl(),
                }
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
            } if self.modifiers.alt => self.show_debug_ui = !self.show_debug_ui,
            Event::DeviceEvent {
                event:
                    DeviceEvent::Key(KeyboardInput {
                        virtual_keycode: Some(VirtualKeyCode::Return),
                        state: ElementState::Pressed,
                        ..
                    }),
                ..
            } if self.modifiers.alt => self.toggle_fullscreen(window),
            Event::DeviceEvent {
                event:
                    DeviceEvent::Key(KeyboardInput {
                        scancode, state, ..
                    }),
                ..
            } => {
                if GameConfig::get().keyboard_buttons {
                    for button in GameConfig::get()
                        .keybinds
                        .iter()
                        .filter_map(|x| x.match_button(*scancode))
                    {
                        if self.input_state.is_button_held(button).is_none()
                            || *state == ElementState::Released
                        {
                            let button = UscInputEvent::Button(
                                button,
                                *state,
                                if offset_neg {
                                    SystemTime::now().add(offset)
                                } else {
                                    SystemTime::now().sub(offset)
                                },
                            );
                            transformed_event = Some(Event::UserEvent(button));
                        }
                    }
                }
            }
            Event::DeviceEvent {
                event: game_loop::winit::event::DeviceEvent::MouseMotion { delta },
                ..
            } if GameConfig::get().mouse_knobs => {
                {
                    //TODO: Move somewhere else?
                    let s = window.inner_size();
                    _ = window
                        .set_cursor_position(PhysicalPosition::new(s.width / 2, s.height / 2));
                }

                let sens = GameConfig::get().mouse_ppr;
                let mut ls = LaserState::default();
                ls.update(kson::Side::Left, (delta.0 / sens) as _);
                ls.update(kson::Side::Right, (delta.1 / sens) as _);

                transformed_event = Some(Event::UserEvent(UscInputEvent::Laser(
                    ls,
                    SystemTime::now().sub(offset),
                )));
            }
            _ => (),
        }

        if let Some(Event::UserEvent(e)) = transformed_event {
            self.input_state.update(&e);
            match e {
                UscInputEvent::Button(b, ElementState::Pressed, time) => self
                    .scenes
                    .for_each_active_mut(|x| x.on_button_pressed(b, time)),
                UscInputEvent::Button(b, ElementState::Released, time) => self
                    .scenes
                    .for_each_active_mut(|x| x.on_button_released(b, time)),
                UscInputEvent::Laser(_, _) => {}
            }
        }

        self.scenes
            .active
            .iter_mut()
            .filter(|x| !x.is_suspended())
            .for_each(|x| x.on_event(transformed_event.as_ref().unwrap_or(event)));
    }

    fn run_lua_gc(
        lua_arena: &mut RefMut<LuaArena>,
        vgfx: &mut Vgfx,
        transition_lua_idx: Index,
        transition_song_lua_idx: Index,
    ) {
        profile_scope!("Garbage collect");
        lua_arena.write().unwrap().0.retain(|idx, lua| {
            //TODO: if reference count = 1, remove loaded gfx assets for state
            //lua.gc_collect();
            if Rc::strong_count(lua) > 1
                || idx == transition_lua_idx
                || idx == transition_song_lua_idx
            {
                LuaHttp::poll(lua);
                true
            } else {
                vgfx.drop_assets(lua_address(lua));
                false
            }
        });
    }

    fn debug_ui(gui_context: &egui::Context, scenes: &mut Scenes) {
        profile_function!();
        if let Some(s) = scenes.active.last_mut() {
            crate::log_result!(s.debug_ui(gui_context));
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
    }

    fn render_overlays(
        vgfx: &Arc<RwLock<Vgfx>>,
        frame_input: &td::FrameInput,
        fps: f64,
        fps_paint: &vg::Paint,
    ) {
        profile_function!();
        let vgfx_lock = vgfx.write();
        if let Ok(vgfx) = vgfx_lock {
            let mut canvas_lock = vgfx.canvas.try_lock();
            if let Ok(ref mut canvas) = canvas_lock {
                canvas.reset();
                _ = canvas.fill_text(
                    frame_input.viewport.width as f32 - 5.0,
                    frame_input.viewport.height as f32 - 5.0,
                    format!("{:.1} FPS", fps),
                    fps_paint,
                );

                {
                    profile_scope!("Flush Canvas");
                    canvas.flush(); //also flushes game game ui, can take longer than it looks like it should
                }
            }
        }
    }

    fn update_game_data_and_clear(
        game_data: &Arc<RwLock<GameData>>,
        mousex: f64,
        mousey: f64,
        frame_input: &td::FrameInput,
        input_state: InputState,
    ) {
        profile_function!();
        {
            let lock = game_data.write();
            if let Ok(mut game_data) = lock {
                *game_data = GameData {
                    mouse_pos: (mousex, mousey),
                    resolution: (frame_input.viewport.width, frame_input.viewport.height),
                    profile_stack: std::mem::take(&mut game_data.profile_stack),
                    input_state,
                    audio_samples: std::mem::take(&mut game_data.audio_samples),
                    audio_sample_play_status: std::mem::take(
                        &mut game_data.audio_sample_play_status,
                    ),
                };
            }
        }

        {
            frame_input
                .screen()
                .clear(td::ClearState::color_and_depth(0.0, 0.0, 0.0, 1.0, 1.0));
            // .render(&camera, [&model], &[]);
        }
    }

    fn reset_viewport_size(vgfx: Arc<RwLock<Vgfx>>, frame_input: &td::FrameInput) {
        let vgfx_lock = vgfx.write();
        if let Ok(vgfx) = vgfx_lock {
            let mut canvas_lock = vgfx.canvas.try_lock();
            if let Ok(ref mut canvas) = canvas_lock {
                canvas.reset();
                canvas.set_size(frame_input.viewport.width, frame_input.viewport.height, 1.0);
                canvas.flush();
            }
        }
    }

    fn toggle_fullscreen(&self, window: &Window) {
        match window.fullscreen() {
            Some(_) => window.set_fullscreen(None),
            None => window.set_fullscreen(Some(game_loop::winit::window::Fullscreen::Borderless(
                window.current_monitor(),
            ))),
        }
    }
}

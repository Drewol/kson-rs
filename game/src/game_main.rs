use std::{
    num::NonZeroU32,
    ops::{Add, Sub},
    rc::Rc,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, RwLock,
    },
    time::{Duration, SystemTime},
};

use di::{RefMut, ServiceProvider};
use egui_glow::{egui_winit::accesskit_winit::WindowEvent, EguiGlow};
use femtovg::Paint;
use log::info;
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event,
    keyboard::{Key, NamedKey, PhysicalKey},
    window::Window,
};

use glutin::{
    context::PossiblyCurrentContext,
    surface::{GlSurface, SwapInterval},
};
use puffin::{profile_function, profile_scope};

use crate::{
    button_codes::UscButton, ir::InternetRanking, lighting::LightingService,
    songselect::SongProviderSelection, touch::TouchHelper, FrameInput,
};
use mlua::Lua;
use td::Modifiers;

use femtovg as vg;
use three_d as td;

use crate::LuaArena;
use crate::{
    button_codes::{LaserState, UscInputEvent},
    companion_interface::{self},
    config::{Fullscreen, GameConfig},
    game::{gauge::Gauge, HitRating},
    game_data::GameData,
    help,
    input_state::InputState,
    lua_http::LuaHttp,
    lua_service::LuaProvider,
    main_menu::MainMenuButton,
    scene,
    settings_screen::SettingsScreen,
    song_provider, songselect,
    transition::Transition,
    util::lua_address,
    vg_ui::Vgfx,
    window::find_monitor,
    worker_service::WorkerService,
    RuscMixer, Scenes, FRAME_ACC_SIZE,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AutoPlay {
    None,
    Buttons,
    Lasers,
    All,
}

impl AutoPlay {
    pub fn any(&self) -> bool {
        !matches!(self, AutoPlay::None)
    }
}

pub enum ControlMessage {
    None,
    MainMenu(MainMenuButton),
    SongSelect(SongProviderSelection),
    Song {
        song: Arc<songselect::Song>,
        diff: usize,
        loader: song_provider::LoadSongFn,
        autoplay: AutoPlay,
    },

    Result {
        song: Arc<songselect::Song>,
        diff_idx: usize,
        score: u32,
        gauge: Gauge,
        hit_ratings: Vec<HitRating>,
        hit_window: crate::game::HitWindow,
        autoplay: AutoPlay,
        max_combo: i32,
        duration: i32,
        manual_exit: bool,
        hash: String,
    },

    ApplySettings,
}

impl Default for ControlMessage {
    fn default() -> Self {
        Self::None
    }
}

pub struct GameMain {
    lua_arena: di::RefMut<LuaArena>,
    lua_provider: Arc<LuaProvider>,
    companion_server: di::RefMut<companion_interface::CompanionServer>,
    companion_update: u8,
    scenes: Scenes,
    pub control_tx: Sender<ControlMessage>,
    control_rx: Receiver<ControlMessage>,
    knob_state: LaserState,
    frame_times: [f64; 16],
    frame_time_index: usize,
    fps_paint: Paint,
    transition_lua: Rc<Lua>,
    transition_song_lua: Rc<Lua>,
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
    show_fps: bool,
    frame_end: std::time::SystemTime,
    frame_duration: Duration,
    touch_tracker: TouchHelper,
    mouse_knobs: bool,
    mouse_locked: bool,
}

fn get_frame_duration(settings: &GameConfig) -> Duration {
    let target_fps = settings.graphics.target_fps as u64;
    if target_fps == 0 {
        Duration::from_nanos(1)
    } else {
        Duration::from_nanos(1_000_000_000 / target_fps.max(30))
    }
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
            companion_server: service_provider.get_required(),
            scenes,
            control_tx,
            control_rx,
            knob_state: LaserState::default(),
            frame_times: [0.01; 16],
            frame_time_index: 0,
            fps_paint,
            transition_lua: LuaProvider::new_lua(),
            transition_song_lua: LuaProvider::new_lua(),
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
            show_fps: GameConfig::get().graphics.show_fps,
            companion_update: 0,
            frame_end: SystemTime::UNIX_EPOCH,
            frame_duration: get_frame_duration(&GameConfig::get()),
            touch_tracker: TouchHelper::new(egui::accesskit::Vec2::new(500.0, 500.0)),
            mouse_knobs: GameConfig::get().mouse_knobs,
            mouse_locked: false,
        }
    }

    const KEYBOARD_LASER_SENS: f32 = 2.0 / 240.0;
    pub fn update(&mut self) {
        self.scenes
            .tick(1000.0 / 240.0, self.knob_state, self.control_tx.clone());

        {
            for ele in self.service_provider.get_all_mut::<dyn WorkerService>() {
                profile_scope!("Worker update");
                ele.write().expect("Worker service closed").update()
            }
        }

        if self.companion_update == 0 {
            profile_scope!("Companion update");
            let server = self.companion_server.read().unwrap();

            if server.active.load(std::sync::atomic::Ordering::Relaxed) {
                let state = self
                    .scenes
                    .active
                    .last()
                    .map(|x| x.game_state())
                    .unwrap_or(companion_interface::GameState::None);
                server.send_state(state);
            }

            self.companion_update = 30; // every 125ms
        }

        self.companion_update -= 1;

        if GameConfig::get().keyboard_knobs || cfg!(target_os = "android") {
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
        window: &winit::window::Window,
        surface: &glutin::surface::Surface<glutin::surface::WindowSurface>,
        gl_context: &PossiblyCurrentContext,
    ) -> bool {
        let GameMain {
            lua_arena,
            scenes,
            control_tx,
            control_rx,
            knob_state,
            frame_times,
            fps_paint,
            transition_lua,
            transition_song_lua,
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
            show_fps,
            companion_server: _,
            companion_update: _,
            frame_end,
            frame_duration,
            touch_tracker,
            mouse_knobs,
            mouse_locked,
        } = self;

        knob_state.zero_deltas();

        for lua in lua_arena.read().expect("Lock error").0.iter() {
            lua.set_app_data(frame_input.clone());
        }
        let _lua_frame_input = frame_input.clone();
        let _lua_mixer = mixer.clone();

        if frame_input.first_frame {
            frame_input.screen().clear(td::ClearState::default());
            let vgfx = vgfx.write().expect("Lock error");
            let mut canvas = vgfx.canvas.lock().expect("Lock error");
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
            return false;
        }
        if *frame_count == 1 {
            lua_provider
                .register_libraries(transition_lua.clone(), "transition.lua")
                .expect("Failed to register lua libraries");

            lua_provider
                .register_libraries(transition_song_lua.clone(), "songtransition.lua")
                .expect("Failed to register lua libraries");
            *frame_count += 1;
        }

        while let Ok(control_msg) = control_rx.try_recv() {
            match control_msg {
                ControlMessage::None => {}
                ControlMessage::SongSelect(song_provider_selection) => {
                    scenes.suspend_top();

                    if let Ok(_arena) = lua_arena.read() {
                        let transition_lua = transition_lua.clone();
                        scenes.transition = Transition::new(
                            transition_lua,
                            ControlMessage::SongSelect(song_provider_selection),
                            vgfx.clone(),
                            frame_input.viewport,
                            service_provider.create_scope(),
                        )
                        .ok()
                    }
                }
                ControlMessage::MainMenu(b) => match b {
                    MainMenuButton::Start => {
                        scenes.suspend_top();

                        if let Ok(_arena) = lua_arena.read() {
                            let transition_lua = transition_lua.clone();
                            scenes.transition = Transition::new(
                                transition_lua,
                                ControlMessage::MainMenu(MainMenuButton::Start),
                                vgfx.clone(),
                                frame_input.viewport,
                                service_provider.create_scope(),
                            )
                            .ok()
                        }
                    }
                    MainMenuButton::Multiplayer => {
                        scenes.suspend_top();
                        scenes.transition = Transition::new(
                            transition_lua.clone(),
                            ControlMessage::MainMenu(MainMenuButton::Multiplayer),
                            vgfx.clone(),
                            frame_input.viewport,
                            service_provider.create_scope(),
                        )
                        .ok();
                    }
                    MainMenuButton::Downloads => {}
                    MainMenuButton::Exit => {
                        scenes.clear();
                    }
                    MainMenuButton::Options => scenes.loaded.push(Box::new(SettingsScreen::new(
                        service_provider.create_scope(),
                        control_tx.clone(),
                        window,
                    ))),
                    _ => {}
                },
                ControlMessage::Song {
                    diff,
                    loader,
                    song,
                    autoplay,
                } => {
                    if let Ok(_arena) = lua_arena.read() {
                        let transition_lua = transition_song_lua.clone();
                        scenes.transition = Transition::new(
                            transition_lua,
                            ControlMessage::Song {
                                diff,
                                loader,
                                song,
                                autoplay,
                            },
                            vgfx.clone(),
                            frame_input.viewport,
                            service_provider.create_scope(),
                        )
                        .ok()
                    }
                }
                ControlMessage::Result {
                    song,
                    diff_idx,
                    score,
                    gauge,
                    hit_ratings,
                    hit_window,
                    autoplay,
                    max_combo,
                    duration,
                    manual_exit,
                    hash,
                } => {
                    if let Ok(_arena) = lua_arena.read() {
                        let transition_lua = transition_lua.clone();
                        scenes.transition = Transition::new(
                            transition_lua,
                            ControlMessage::Result {
                                song,
                                diff_idx,
                                score,
                                gauge,
                                hit_ratings,
                                hit_window,
                                autoplay,
                                max_combo,
                                duration,
                                manual_exit,
                                hash,
                            },
                            vgfx.clone(),
                            frame_input.viewport,
                            service_provider.create_scope(),
                        )
                        .ok()
                    }
                }
                ControlMessage::ApplySettings => {
                    //TODO: Reload skin
                    let settings = GameConfig::get();
                    _ = surface.set_swap_interval(
                        gl_context,
                        if settings.graphics.vsync {
                            SwapInterval::Wait(NonZeroU32::new(1).expect("Invalid value"))
                        } else {
                            SwapInterval::DontWait
                        },
                    );

                    *show_fps = settings.graphics.show_fps;
                    *mouse_knobs = settings.mouse_knobs;

                    *frame_duration = get_frame_duration(&settings);

                    window.set_fullscreen(match settings.graphics.fullscreen {
                        Fullscreen::Windowed { .. } => None,
                        Fullscreen::Borderless { monitor } => {
                            let m = find_monitor(window.available_monitors(), monitor);
                            Some(winit::window::Fullscreen::Borderless(m))
                        }
                        Fullscreen::Exclusive {
                            monitor,
                            resolution,
                        } => {
                            let m =
                                find_monitor(window.available_monitors(), monitor).and_then(|m| {
                                    m.video_modes()
                                        .filter(|x| x.size() == resolution)
                                        .max_by_key(|x| x.refresh_rate_millihertz())
                                });

                            m.map(winit::window::Fullscreen::Exclusive)
                        }
                    });

                    let sink = service_provider.get_required::<rodio::Sink>();
                    sink.set_volume(settings.master_volume);

                    service_provider
                        .get_required_mut::<LightingService>()
                        .write()
                        .unwrap()
                        .restart();

                    settings.save();
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
            *mouse_knobs,
        );

        scenes.render(frame_input.clone(), vgfx);
        Self::render_overlays(vgfx, &frame_input, fps, fps_paint, *show_fps);

        gui.run(window, |ctx| {
            scenes.render_egui(ctx);

            if *show_debug_ui {
                Self::debug_ui(ctx, scenes, vgfx);
            }
        });
        gui.paint(window);

        Self::run_lua_gc(lua_arena, &mut vgfx.write().expect("Lock error"));

        if let Ok(mut a) = game_data.write() {
            a.profile_stack.clear()
        }

        let exit = scenes.is_empty();
        if exit {
            GameConfig::get().save()
        }

        let lock_mouse = !gui.egui_ctx.is_pointer_over_area()
            && *mouse_knobs
            && !*show_debug_ui
            && window.has_focus()
            && !self.input_state.text_input_active();

        if lock_mouse != *mouse_locked {
            *mouse_locked = lock_mouse;
            if lock_mouse {
                let s = window.inner_size();
                _ = window.set_cursor_position(PhysicalPosition::new(s.width / 2, s.height / 2));
                _ = window.set_cursor_grab(winit::window::CursorGrabMode::Locked);
                window.set_cursor_visible(false);
            } else {
                window.set_cursor_visible(true);
                _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
            }
        }

        {
            profile_scope!("Wait on FPS limiter");
            crate::help::wait_until(*frame_end);
            *frame_end = SystemTime::now() + *frame_duration;
        }

        exit
    }
    pub fn handle(&mut self, window: &Window, event: &winit::event::Event<UscInputEvent>) {
        use winit::event::*;
        if let Event::WindowEvent {
            window_id: _,
            event,
        } = event
        {
            if self.show_debug_ui || self.scenes.should_render_egui() {
                let event_response = self.gui.on_window_event(window, event);
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
        let text_input_active = self.input_state.text_input_active();

        //TODO: Refactor keyboard handling
        match event {
            Event::UserEvent(e) => {
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
                    UscInputEvent::ClientEvent(_) => {}
                }
            }
            Event::WindowEvent {
                window_id: _,
                event: WindowEvent::Resized(physical_size),
            } => {
                let windowed = &mut GameConfig::get_mut().graphics.fullscreen;
                if let Fullscreen::Windowed { size, .. } = windowed {
                    *size = *physical_size;
                }
                self.touch_tracker = TouchHelper::new(egui::accesskit::Vec2::new(
                    physical_size.width as f64,
                    physical_size.height as f64,
                ));

                self.reset_viewport_size(physical_size)
            }
            Event::WindowEvent {
                window_id: _,
                event: WindowEvent::Moved(physical_pos),
            } => {
                let windowed = &mut GameConfig::get_mut().graphics.fullscreen;
                if let Fullscreen::Windowed { pos, .. } = windowed {
                    *pos = *physical_pos;
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
                window_id,
                event: WindowEvent::Touch(t),
            } => {
                info!("{:?}", t);
                if let Some((e, opt)) = self.touch_tracker.update(t) {
                    info!("{:?}, {:?}", e, opt);
                    self.handle(window, &Event::UserEvent(e));
                    if let Some(e) = opt {
                        self.handle(window, &Event::UserEvent(e));
                    }
                }
            }

            Event::WindowEvent {
                event: WindowEvent::ModifiersChanged(mods),
                ..
            } => {
                self.modifiers = three_d::renderer::control::Modifiers {
                    alt: mods.state().alt_key(),
                    ctrl: mods.state().control_key(),
                    shift: mods.state().shift_key(),
                    command: mods.state().super_key(),
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => self.scenes.clear(),
            Event::WindowEvent {
                event: WindowEvent::KeyboardInput { event: key, .. },
                ..
            } if key.state == ElementState::Pressed
                && key.physical_key == PhysicalKey::Code(winit::keyboard::KeyCode::KeyD)
                && self.modifiers.alt
                && !text_input_active =>
            {
                self.show_debug_ui = !self.show_debug_ui
            }
            Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        event:
                            KeyEvent {
                                logical_key: Key::Named(NamedKey::Enter),
                                state: ElementState::Pressed,
                                ..
                            },
                        ..
                    },
                ..
            } if self.modifiers.alt && !text_input_active => self.toggle_fullscreen(window),
            Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        event:
                            KeyEvent {
                                physical_key,
                                state,
                                logical_key,
                                ..
                            },
                        ..
                    },
                ..
            } => {
                if !text_input_active {
                    for button in GameConfig::get()
                        .keybinds
                        .iter()
                        .filter_map(|x| x.match_button(*physical_key))
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

                    #[cfg(target_os = "android")]
                    {
                        let button = match logical_key {
                            Key::Named(NamedKey::BrowserBack) => Some(UscButton::Back),
                            //TODO: Figure out gamepad input
                            _ => None,
                        };

                        if let Some(btn) = button {
                            transformed_event = Some(Event::UserEvent(UscInputEvent::Button(
                                btn,
                                *state,
                                SystemTime::now(),
                            )));
                        }
                    }
                }
            }
            Event::DeviceEvent {
                event: winit::event::DeviceEvent::MouseMotion { delta },
                ..
            } if !text_input_active && GameConfig::get().mouse_knobs => {
                let sens = GameConfig::get().mouse_ppr as f32;
                let mut state = self.input_state.clone_laser();
                state.zero_deltas();
                state.update_delta(kson::Side::Left, delta.0 as f32 / sens);
                state.update_delta(kson::Side::Right, delta.1 as f32 / sens);

                transformed_event = Some(Event::UserEvent(UscInputEvent::Laser(
                    state,
                    SystemTime::now(),
                )));
            }
            _ => (),
        }

        if let Some(Event::UserEvent(e)) = transformed_event.as_ref() {
            self.input_state.update(e);
            match e {
                UscInputEvent::Button(b, ElementState::Pressed, time) => self
                    .scenes
                    .for_each_active_mut(|x| x.on_button_pressed(*b, *time)),
                UscInputEvent::Button(b, ElementState::Released, time) => self
                    .scenes
                    .for_each_active_mut(|x| x.on_button_released(*b, *time)),
                UscInputEvent::Laser(_, _) => {}
                UscInputEvent::ClientEvent(_) => {}
            }
        }

        self.scenes
            .active
            .iter_mut()
            .filter(|x| !x.is_suspended())
            .for_each(|x| x.on_event(transformed_event.as_ref().unwrap_or(event)));
    }

    fn run_lua_gc(lua_arena: &mut RefMut<LuaArena>, vgfx: &mut Vgfx) {
        profile_scope!("Garbage collect");
        lua_arena.write().expect("Lock error").0.retain(|lua| {
            //lua.gc_collect();
            if Rc::strong_count(lua) > 1 {
                LuaHttp::poll(lua);
                InternetRanking::poll(lua);
                true
            } else {
                vgfx.drop_assets(lua_address(lua));
                false
            }
        });
    }

    fn debug_ui(gui_context: &egui::Context, scenes: &mut Scenes, vgfx: &Arc<RwLock<Vgfx>>) {
        profile_function!();
        if let Some(s) = scenes.active.last_mut() {
            crate::log_result!(s.debug_ui(gui_context));
        }
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

            if ui.button("Take screenshot").clicked() {
                match help::take_screenshot(&vgfx.read().unwrap(), None) {
                    Ok(p) => {
                        log::info!("Saved screenshot to: {p:?}")
                    }
                    Err(e) => {
                        log::warn!("Failed to save screenshot: {e}")
                    }
                }
            }
        });
    }

    fn render_overlays(
        vgfx: &Arc<RwLock<Vgfx>>,
        frame_input: &crate::FrameInput,
        fps: f64,
        fps_paint: &vg::Paint,
        show_fps: bool,
    ) {
        profile_function!();
        let vgfx_lock = vgfx.write();
        if let Ok(vgfx) = vgfx_lock {
            let mut canvas_lock = vgfx.canvas.try_lock();
            if let Ok(ref mut canvas) = canvas_lock {
                canvas.reset();
                if show_fps {
                    _ = canvas.fill_text(
                        frame_input.viewport.width as f32 - 5.0,
                        frame_input.viewport.height as f32 - 5.0,
                        format!("{:.1} FPS", fps),
                        fps_paint,
                    );
                }

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
        frame_input: &crate::FrameInput,
        input_state: InputState,
        mouse_knobs: bool,
    ) {
        profile_function!();
        {
            let lock = game_data.write();
            if let Ok(mut game_data) = lock {
                *game_data = GameData {
                    mouse_pos: if mouse_knobs {
                        (-1.0, -1.0)
                    } else {
                        (mousex, mousey)
                    },
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

    fn reset_viewport_size(&self, size: &PhysicalSize<u32>) {
        let vgfx_lock = self.vgfx.write();
        if let Ok(vgfx) = vgfx_lock {
            let mut canvas_lock = vgfx.canvas.try_lock();
            if let Ok(ref mut canvas) = canvas_lock {
                canvas.reset();
                canvas.set_size(size.width, size.height, 1.0);
                canvas.flush();
            }
        }
    }

    fn toggle_fullscreen(&self, window: &Window) {
        let fullscreen = &mut GameConfig::get_mut().graphics.fullscreen;
        match window.fullscreen() {
            Some(_) => {
                window.set_fullscreen(None);
                *fullscreen = Fullscreen::Windowed {
                    pos: window
                        .outer_position()
                        .unwrap_or(PhysicalPosition::new(0, 0)),
                    size: window.inner_size(),
                }
            }
            None => {
                let current_monitor = window.current_monitor();

                if let Some(m) = current_monitor.as_ref() {
                    *fullscreen = Fullscreen::Borderless {
                        monitor: m.position(),
                    };
                }

                window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(current_monitor)))
            }
        }
    }
}

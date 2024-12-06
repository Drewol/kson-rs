use std::{
    path::PathBuf,
    rc::Rc,
    sync::{mpsc::Sender, Arc},
};

use anyhow::anyhow;
use di::{RefMut, ServiceProvider};

use glam::{vec2, Mat4};
use log::warn;
use poll_promise::Promise;
use rodio::Source;
use serde_json::json;
use tealr::mlu::mlua::{Function, Lua, LuaSerdeExt};
use wgpu::{Origin3d, SurfaceTexture, Texture};

use crate::{
    game_main::AutoPlay,
    help::RenderContext,
    log_result,
    main_menu::MainMenuButton,
    results::SongResultData,
    scene::{Scene, SceneData},
    shaded_mesh::ShadedMesh,
    songselect::{Song, SongSelect},
    util::{back_pixels, lua_address},
    ControlMessage, Viewport,
};

#[derive(Debug, PartialEq, Eq)]
pub enum TransitionState {
    Intro,
    Loading,
    Countdown(u8), //TODO: Just a workaround because i'm stupid
    Outro,
    Done,
}

pub struct Transition {
    target: ControlMessage,
    target_state: Option<Promise<anyhow::Result<Box<dyn SceneData + Send>>>>,
    control_tx: Sender<ControlMessage>,
    pub state: TransitionState,
    transition_lua: Rc<Lua>,
    vgfx: RefMut<crate::Vgfx>,
    prev_screengrab: Arc<Texture>,
    service_provider: ServiceProvider,
}

fn load_songs() -> anyhow::Result<Box<dyn SceneData + Send>> {
    Ok(Box::new(SongSelect::new()))
}

fn load_chart(
    chart: kson::Chart,
    song: Arc<Song>,
    diff_idx: usize,
    skin_folder: PathBuf,
    audio: Box<dyn Source<Item = f32> + Send>,
    autoplay: AutoPlay,
) -> anyhow::Result<Box<dyn SceneData + Send>> {
    Ok(Box::new(crate::game::GameData::new(
        song,
        diff_idx,
        chart,
        skin_folder,
        audio,
        autoplay,
    )?))
}

impl Transition {
    pub fn do_outro(&mut self) {
        self.state = TransitionState::Countdown(5);
    }

    pub fn new(
        transition_lua: Rc<Lua>,
        target: ControlMessage,
        control_tx: Sender<ControlMessage>,
        vgfx: RefMut<crate::Vgfx>,
        viewport: Viewport,
        surface: &SurfaceTexture,

        service_provider: ServiceProvider,
    ) -> anyhow::Result<Self> {
        if let Ok(reset_fn) = transition_lua.globals().get::<_, Function>("reset") {
            if let Some(e) = reset_fn.call::<(), ()>(()).err() {
                warn!("Error resetting transition: {}", e);
            };
        }

        let context = service_provider
            .get_required::<RenderContext>()
            .as_ref()
            .clone();

        let prev_grab = screen_grab(&context, viewport, surface);

        if let ControlMessage::Song { song, diff, .. } = &target {
            let mut vgfx = vgfx.write().expect("Failed to lock VG");
            let diff = song
                .difficulties
                .read()
                .expect("Failed to lock song diffs")
                .get(*diff)
                .cloned()
                .ok_or(anyhow!("Song does not contain selected diff"))?;
            let lua_idx = lua_address(&transition_lua);
            log_result!(transition_lua.globals().set(
                "song",
                transition_lua.to_value(&json!({
                    "jacket": vgfx.load_image(&diff.jacket_path, lua_idx).unwrap_or(0),
                    "title": song.title,
                    "artist": song.artist,
                    "bpm": song.bpm,
                    "difficulty": diff.difficulty,
                    "level": diff.level,
                    "effector": diff.effector
                }))?
            ));
        }

        Ok(Self {
            target,
            transition_lua,
            target_state: None,
            control_tx,
            state: TransitionState::Intro,
            vgfx,
            prev_screengrab: prev_grab,
            service_provider,
        })
    }
}

pub fn screen_grab(
    context: &RenderContext,
    viewport: Viewport,
    surface: &SurfaceTexture,
) -> Arc<wgpu::Texture> {
    context.new_screen_texture(viewport, surface)
}

impl Scene for Transition {
    fn tick(
        &mut self,
        _dt: f64,
        _knob_state: crate::button_codes::LaserState,
    ) -> anyhow::Result<()> {
        if self.state == TransitionState::Loading && self.target_state.is_none() {
            self.state = TransitionState::Countdown(5)
        }

        Ok(())
    }

    fn render(
        &mut self,
        _dt: f64,
        context: &RenderContext,
        viewport: Viewport,
        s: &SurfaceTexture,
    ) {
        match self.state {
            TransitionState::Intro => {
                let mut encoder = context.encoder(None);
                encoder.copy_texture_to_texture(
                    wgpu::ImageCopyTextureBase {
                        texture: &self.prev_screengrab,
                        mip_level: 0,
                        origin: Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::ImageCopyTextureBase {
                        texture: &s.texture,
                        mip_level: 0,
                        origin: Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    viewport.extend3d(1),
                );

                context.queue.submit([encoder.finish()]);
            }
            TransitionState::Countdown(0) => self.state = TransitionState::Outro,
            TransitionState::Countdown(c) => self.state = TransitionState::Countdown(c - 1),
            _ => (),
        }
    }

    fn render_ui(&mut self, dt: f64) -> anyhow::Result<()> {
        {
            self.vgfx
                .read()
                .expect("Lock error")
                .canvas
                .lock()
                .expect("Lock error")
                .reset();
        }
        //TODO: Render last frame before transition
        //TODO: Handle rendering of next scene during outro
        match self.state {
            TransitionState::Intro => {
                let render: Function = self.transition_lua.globals().get("render")?;
                let intro_complete: bool = render.call(dt / 1000_f64)?;

                if TransitionState::Intro == self.state && intro_complete {
                    self.state = TransitionState::Loading;
                    let target = std::mem::take(&mut self.target);

                    self.target_state = match target {
                        ControlMessage::MainMenu(MainMenuButton::Start) => {
                            Some(Promise::spawn_thread("Load song select", move || {
                                load_songs()
                            }))
                        }
                        ControlMessage::Song {
                            song,
                            diff,
                            loader,
                            autoplay,
                        } => {
                            let skin_folder = self.vgfx.read().expect("Lock error").skin_folder();
                            Some(Promise::spawn_thread("Load song", move || {
                                let (chart, audio) = loader()?;
                                load_chart(chart, song, diff, skin_folder, audio, autoplay)
                            }))
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
                        } => Some(Promise::spawn_thread(
                            "Load song",
                            move || -> anyhow::Result<Box<dyn SceneData + Send>> {
                                Ok(Box::new(SongResultData::from_diff(
                                    song,
                                    diff_idx,
                                    score,
                                    hit_ratings,
                                    gauge,
                                    hit_window,
                                    autoplay,
                                    max_combo,
                                    duration,
                                    manual_exit,
                                )?))
                            },
                        )),
                        _ => None,
                    }
                }
            }
            TransitionState::Loading | TransitionState::Countdown(_) => {
                let render: Function = self.transition_lua.globals().get("render")?;
                render.call(dt / 1000_f64)?;
                if let Some(target_state) = self.target_state.take() {
                    match target_state.try_take() {
                        Ok(Ok(finished)) => self
                            .control_tx
                            .send(ControlMessage::TransitionComplete(
                                finished.make_scene(self.service_provider.create_scope())?,
                            ))
                            .expect("Failed to communicate with main game"),
                        Ok(Err(loading_error)) => {
                            log::error!("{}", loading_error);
                            self.state = TransitionState::Countdown(5);
                        }
                        Err(loading) => self.target_state = Some(loading),
                    }
                }
            }
            TransitionState::Outro => {
                let render: Function = self.transition_lua.globals().get("render_out")?;
                let outro_complete: bool = render.call((dt / 1000_f64).min(0.1))?;
                if outro_complete {
                    self.state = TransitionState::Done;
                }
            }

            TransitionState::Done => {}
        }

        Ok(())
    }

    fn is_suspended(&self) -> bool {
        false
    }

    fn debug_ui(&mut self, _ctx: &egui::Context) -> anyhow::Result<()> {
        Ok(())
    }

    fn closed(&self) -> bool {
        self.state == TransitionState::Done
    }

    fn name(&self) -> &str {
        "Transition"
    }
}

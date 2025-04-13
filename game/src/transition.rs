use std::{
    path::PathBuf,
    rc::Rc,
    sync::{mpsc::Sender, Arc},
};

use anyhow::anyhow;
use di::{RefMut, ServiceProvider};

use log::warn;
use mlua::{Function, Lua, LuaSerdeExt};
use poll_promise::Promise;
use rodio::Source;
use serde_json::json;
use three_d::{ColorMaterial, Gm, Mat3, Rad, Rectangle, Texture2DRef, Vec2, Zero};

use crate::{
    game_main::AutoPlay,
    log_result,
    main_menu::MainMenuButton,
    results::SongResultData,
    scene::{Scene, SceneData},
    songselect::{Song, SongSelect},
    util::{back_pixels, lua_address},
    ControlMessage,
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
    prev_screengrab: Option<Gm<Rectangle, ColorMaterial>>,
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
    chart_folder: Option<PathBuf>,
) -> anyhow::Result<Box<dyn SceneData + Send>> {
    Ok(Box::new(crate::game::GameData::new(
        song,
        diff_idx,
        chart,
        skin_folder,
        audio,
        autoplay,
        chart_folder,
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
        viewport: three_d::Viewport,

        service_provider: ServiceProvider,
    ) -> anyhow::Result<Self> {
        if let Ok(reset_fn) = transition_lua.globals().get::<Function>("reset") {
            if let Some(e) = reset_fn.call::<()>(()).err() {
                warn!("Error resetting transition: {}", e);
            };
        }

        let context = service_provider
            .get_required::<three_d::Context>()
            .as_ref()
            .clone();

        let prev_grab = screen_grab(context, viewport);

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
    context: three_d::Context,
    viewport: three_d::Viewport,
) -> Option<Gm<Rectangle, ColorMaterial>> {
    let screen_tex = three_d::texture::CpuTexture {
        data: three_d::TextureData::RgbaU8(back_pixels(&context, viewport)),
        height: viewport.height,
        width: viewport.width,
        ..Default::default()
    };

    Some(three_d::Gm::new(
        Rectangle::new(&context, Vec2::zero(), Rad::zero(), 1.0, 1.0),
        three_d::ColorMaterial {
            texture: Some(Texture2DRef {
                texture: Arc::new(three_d::Texture2D::new(&context, &screen_tex)),
                transformation: Mat3::from_nonuniform_scale(1.0, -1.0),
            }),
            color: three_d::Srgba::WHITE,
            ..Default::default()
        },
    ))
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
        _td_context: &three_d::Context,
        target: &mut three_d::RenderTarget,
        viewport: three_d::Viewport,
    ) {
        use three_d::*;

        match self.state {
            TransitionState::Intro => {
                if let Some(screengrab) = &mut self.prev_screengrab {
                    screengrab.set_size(viewport.width as f32, viewport.height as f32);
                    screengrab.set_center(vec2(
                        viewport.width as f32 / 2.0,
                        viewport.height as f32 / 2.0,
                    ));
                    let mut new_2d = Camera::new_2d(viewport);
                    new_2d.disable_tone_and_color_mapping();
                    target.render(&new_2d, &[screengrab], &[]);
                }
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
                                let (chart, audio, chart_path) = loader()?;
                                load_chart(
                                    chart,
                                    song,
                                    diff,
                                    skin_folder,
                                    audio,
                                    autoplay,
                                    chart_path,
                                )
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

use std::{
    path::PathBuf,
    rc::Rc,
    sync::{mpsc::Sender, Arc, Mutex},
};

use generational_arena::Index;
use poll_promise::Promise;
use tealr::mlu::mlua::{Function, Lua, LuaSerdeExt};
use three_d::{ColorMaterial, Gm, Mat3, Rad, Rectangle, Texture2DRef, Vec2, Zero};
use ureq::json;

use crate::{
    main_menu::MainMenuButton,
    results::SongResultData,
    scene::{Scene, SceneData},
    songselect::{Song, SongSelect},
    util::back_pixels,
    ControlMessage,
};

#[derive(Debug, PartialEq, Eq)]
pub enum TransitionState {
    Intro,
    Loading,
    Outro,
    Done,
}

pub struct Transition {
    target: ControlMessage,
    target_state: Option<Promise<anyhow::Result<Box<dyn SceneData + Send>>>>,
    control_tx: Sender<ControlMessage>,
    pub state: TransitionState,
    transition_lua: Rc<Lua>,
    context: three_d::Context,
    vgfx: Arc<Mutex<crate::Vgfx>>,
    prev_screengrab: Option<Gm<Rectangle, ColorMaterial>>,
}

fn load_songs() -> anyhow::Result<Box<dyn SceneData + Send>> {
    //TODO: Global config object?
    // Song databse?
    // Song provider?
    Ok(Box::new(SongSelect::new()))
}

fn load_chart(
    context: three_d::Context,
    chart: kson::Chart,
    song: Arc<Song>,
    diff_idx: usize,
    skin_folder: PathBuf,
) -> anyhow::Result<Box<dyn SceneData + Send>> {
    Ok(Box::new(crate::game::GameData::new(
        context,
        song,
        diff_idx,
        chart,
        skin_folder,
    )?))
}

impl Transition {
    pub fn do_outro(&mut self) {
        self.state = TransitionState::Outro;
    }

    pub fn new(
        transition_lua: Rc<Lua>,
        target: ControlMessage,
        control_tx: Sender<ControlMessage>,
        context: three_d::Context,
        vgfx: Arc<Mutex<crate::Vgfx>>,
        viewport: three_d::Viewport,
    ) -> Self {
        if let Ok(reset_fn) = transition_lua.globals().get::<_, Function>("reset") {
            reset_fn.call::<(), ()>(());
        }

        let prev_grab = {
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
                        transformation: Mat3::from_scale(1.0),
                    }),
                    color: three_d::Color::WHITE,
                    ..Default::default()
                },
            ))
        };

        if let ControlMessage::Song {
            song,
            diff,
            loader: _,
        } = &target
        {
            let mut vgfx = vgfx.lock().unwrap();
            let diff = &song.difficulties[*diff];
            let lua_idx = transition_lua.app_data_ref::<Index>().unwrap();
            transition_lua.globals().set(
                "song",
                transition_lua
                    .to_value(&json!({
                        "jacket": vgfx.load_image(&diff.jacket_path, &lua_idx).unwrap_or(0),
                        "title": song.title,
                        "artist": song.artist,
                        "bpm": song.bpm,
                        "difficulty": diff.difficulty,
                        "level": diff.level,
                        "effector": diff.effector
                    }))
                    .unwrap(),
            );
        }

        Self {
            target,
            transition_lua,
            target_state: None,
            control_tx,
            state: TransitionState::Intro,
            context,
            vgfx,
            prev_screengrab: prev_grab,
        }
    }
}

impl Scene for Transition {
    fn tick(
        &mut self,
        _dt: f64,
        _knob_state: crate::button_codes::LaserState,
    ) -> anyhow::Result<()> {
        if self.state == TransitionState::Loading && self.target_state.is_none() {
            self.state = TransitionState::Outro
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

        if let TransitionState::Intro = self.state {
            if let Some(screengrab) = &mut self.prev_screengrab {
                screengrab.set_size(viewport.width as f32, viewport.height as f32);
                screengrab.set_center(vec2(
                    viewport.width as f32 / 2.0,
                    viewport.height as f32 / 2.0,
                ));
                target.render(&camera2d(viewport), screengrab.into_iter(), &[]);
            }
        }
    }

    fn render_ui(&mut self, dt: f64) -> anyhow::Result<()> {
        {
            self.vgfx.lock().unwrap().canvas.lock().unwrap().reset();
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
                            Some(Promise::spawn_thread("Load song select", load_songs))
                        }
                        ControlMessage::Song { song, diff, loader } => {
                            let context = self.context.clone();
                            let skin_folder = self.vgfx.lock().unwrap().skin_folder();
                            Some(Promise::spawn_thread("Load song", move || {
                                let (chart, _audio) = loader();
                                load_chart(context, chart, song, diff, skin_folder)
                            }))
                        }
                        ControlMessage::Result {
                            song,
                            diff_idx,
                            score,
                            gauge: _,
                        } => Some(Promise::spawn_thread(
                            "Load song",
                            move || -> anyhow::Result<Box<dyn SceneData + Send>> {
                                Ok(Box::new(SongResultData::from_diff(song, diff_idx, score)))
                            },
                        )),
                        _ => None,
                    }
                }
            }
            TransitionState::Loading => {
                let render: Function = self.transition_lua.globals().get("render")?;
                render.call(dt / 1000_f64)?;
                if let Some(target_state) = self.target_state.take() {
                    match target_state.try_take() {
                        Ok(Ok(finished)) => self
                            .control_tx
                            .send(ControlMessage::TransitionComplete(finished.make_scene()))
                            .unwrap(),
                        Ok(Err(loading_error)) => {
                            log::error!("{:?}", loading_error);
                            self.state = TransitionState::Outro;
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

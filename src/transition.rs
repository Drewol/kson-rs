use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::{mpsc::Sender, Arc, Mutex},
};

use femtovg::{rgb::ComponentSlice, Canvas};
use poll_promise::Promise;
use tealr::mlu::mlua::{Function, Lua, LuaSerdeExt};
use three_d::{
    camera2d, vec3, Camera, ColorMaterial, FrameInput, Gm, HasContext, Matrix4, Mesh, Rad,
    Rectangle, RenderTarget, Texture2D, Vec2, Vec3, Zero,
};
use ureq::json;

use crate::{
    config::GameConfig,
    main_menu::MainMenuButton,
    scene::{Scene, SceneData},
    songselect::{SongSelect, SongSelectScene},
    util::back_pixels,
    ControlMessage,
};

#[derive(Debug, PartialEq, Eq)]
enum TransitionState {
    Intro,
    Loading,
    Outro,
    Done,
}

pub struct Transition {
    target: ControlMessage,
    target_state: Option<Promise<anyhow::Result<Box<dyn SceneData + Send>>>>,
    control_tx: Sender<ControlMessage>,
    state: TransitionState,
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
    skin_folder: PathBuf,
) -> anyhow::Result<Box<dyn SceneData + Send>> {
    Ok(Box::new(crate::game::GameData::new(
        context,
        chart,
        skin_folder,
    )?))
}

impl Transition {
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
                    texture: Some(Arc::new(three_d::Texture2D::new(&context, &screen_tex))),
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

            transition_lua.globals().set(
                "song",
                transition_lua
                    .to_value(&json!({
                        "jacket": vgfx.load_image(&diff.jacket_path).unwrap_or(0),
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
    fn tick(&mut self, dt: f64, knob_state: crate::button_codes::LaserState) -> anyhow::Result<()> {
        Ok(())
    }

    fn render(
        &mut self,
        dt: f64,
        td_context: &three_d::Context,
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
                                let (chart, audio) = loader();
                                load_chart(context, chart, skin_folder)
                            }))
                        }
                        _ => None,
                    }
                }
            }
            TransitionState::Loading => {
                let render: Function = self.transition_lua.globals().get("render")?;
                render.call(dt / 1000_f64)?;
                if let Some(target_state) = self.target_state.take() {
                    match target_state.try_take() {
                        Ok(Ok(finished)) => {
                            self.state = TransitionState::Outro;
                            self.control_tx
                                .send(ControlMessage::TransitionComplete(finished.make_scene()))
                                .unwrap()
                        }
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
                let outro_complete: bool = render.call((dt / 1000_f64).max(0.05))?;
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

    fn debug_ui(&mut self, ctx: &three_d::egui::Context) -> anyhow::Result<()> {
        Ok(())
    }

    fn closed(&self) -> bool {
        self.state == TransitionState::Done
    }

    fn name(&self) -> &str {
        "Transition"
    }
}

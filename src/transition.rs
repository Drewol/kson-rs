use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::{mpsc::Sender, Arc, Mutex},
};

use femtovg::Canvas;
use poll_promise::Promise;
use tealr::mlu::mlua::{Function, Lua, LuaSerdeExt};
use three_d::FrameInput;
use ureq::json;

use crate::{
    config::GameConfig,
    main_menu::MainMenuButton,
    scene::{Scene, SceneData},
    songselect::{SongSelect, SongSelectScene},
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
    target_state: Option<Promise<Box<dyn SceneData + Send>>>,
    control_tx: Sender<ControlMessage>,
    state: TransitionState,
    transition_lua: Rc<Lua>,
    context: three_d::Context,
    vgfx: Arc<Mutex<crate::Vgfx>>,
}

fn load_songs() -> Box<dyn SceneData + Send> {
    //TODO: Global config object?
    // Song databse?
    // Song provider?
    Box::new(SongSelect::new())
}

fn load_chart(
    context: three_d::Context,
    chart: kson::Chart,
    skin_folder: PathBuf,
) -> Box<dyn SceneData + Send> {
    Box::new(crate::game::GameData {
        chart,
        skin_folder,
        context,
    })
}

impl Transition {
    pub fn new(
        transition_lua: Rc<Lua>,
        target: ControlMessage,
        control_tx: Sender<ControlMessage>,
        context: three_d::Context,
        vgfx: Arc<Mutex<crate::Vgfx>>,
    ) -> Self {
        if let Ok(reset_fn) = transition_lua.globals().get::<_, Function>("reset") {
            reset_fn.call::<(), ()>(());
        }

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
        }
    }
}

impl Scene for Transition {
    fn tick(&mut self, dt: f64, knob_state: crate::button_codes::LaserState) -> anyhow::Result<()> {
        if let Some(loading) = self.target_state.as_mut() {
            if loading.poll().is_ready() {
                self.state = TransitionState::Outro;
            }
        }

        Ok(())
    }

    fn render_ui(&mut self, dt: f64) -> anyhow::Result<()> {
        //TODO: Render last frame before transition
        //TODO: Handle rendering of next scene during outro
        match self.state {
            TransitionState::Intro | TransitionState::Loading => {
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
            TransitionState::Outro => {
                let render: Function = self.transition_lua.globals().get("render_out")?;
                let outro_complete: bool = render.call(dt / 1000_f64)?;
                if outro_complete {
                    self.state = TransitionState::Done;
                    if let Some(target_state) = self.target_state.take() {
                        if let Ok(scene_data) = target_state.try_take() {
                            self.control_tx
                                .send(ControlMessage::TransitionComplete(scene_data));
                        }
                    }
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

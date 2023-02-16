use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::{mpsc::Sender, Arc, Mutex},
};

use poll_promise::Promise;
use tealr::mlu::mlua::{Function, Lua};

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
}

pub struct Transition {
    target: ControlMessage,
    target_state: Option<Promise<Arc<dyn SceneData + Send>>>,
    control_tx: Sender<ControlMessage>,
    state: TransitionState,
    transition_lua: Rc<Lua>,
}

fn load_songs() -> Arc<dyn SceneData + Send> {
    //TODO: Global config object?
    // Song databse?
    // Song provider?
    Arc::new(SongSelect::new(&GameConfig::get().unwrap().songs_path))
}

impl Transition {
    pub fn new(
        transition_lua: Rc<Lua>,
        target: ControlMessage,
        control_tx: Sender<ControlMessage>,
    ) -> Self {
        if let Ok(reset_fn) = transition_lua.globals().get::<_, Function>("reset") {
            reset_fn.call::<(), ()>(());
        }

        Self {
            target,
            transition_lua,
            target_state: None,
            control_tx,
            state: TransitionState::Intro,
        }
    }
}

impl Scene for Transition {
    fn tick(
        &mut self,
        dt: f64,
        knob_state: crate::button_codes::LaserState,
    ) -> anyhow::Result<bool> {
        if let Some(loading) = self.target_state.as_mut() {
            if loading.poll().is_ready() {
                self.state = TransitionState::Outro;
            }
        }

        Ok(false)
    }

    fn render(&mut self, dt: f64) -> anyhow::Result<bool> {
        //TODO: Render last frame before transition
        //TODO: Handle rendering of next scene during outro
        match self.state {
            TransitionState::Intro | TransitionState::Loading => {
                let render: Function = self.transition_lua.globals().get("render")?;
                let intro_complete: bool = render.call(dt / 1000_f64)?;

                if TransitionState::Intro == self.state && intro_complete {
                    self.state = TransitionState::Loading;
                    self.target_state = match self.target {
                        ControlMessage::MainMenu(MainMenuButton::Start) => {
                            Some(Promise::spawn_thread("Load song select", load_songs))
                        }
                        _ => None,
                    }
                }
                Ok(false)
            }
            TransitionState::Outro => {
                let render: Function = self.transition_lua.globals().get("render_out")?;
                let outro_complete: bool = render.call(dt / 1000_f64)?;
                if outro_complete {
                    if let Some(target_state) = self.target_state.take() {
                        if let Ok(scene_data) = target_state.try_take() {
                            self.control_tx
                                .send(ControlMessage::TransitionComplete(scene_data))?;
                        }
                    }

                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        }
    }

    fn is_suspended(&self) -> bool {
        false
    }

    fn debug_ui(&mut self, ctx: &three_d::egui::Context) -> anyhow::Result<()> {
        Ok(())
    }
}

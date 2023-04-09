use std::{
    path::Path,
    rc::Rc,
    sync::{mpsc::Sender, Arc},
};

use anyhow::Result;
use generational_arena::Index;
use tealr::mlu::mlua::Lua;
use three_d::{ColorMaterial, Event, Gm, Mesh, RenderTarget, Viewport};

use crate::{
    button_codes::{LaserState, UscButton},
    game_data::GameData,
    ControlMessage,
};

#[allow(unused_variables)]
pub trait Scene {
    fn init(
        &mut self,
        load_lua: Box<dyn Fn(Rc<Lua>, &'static str) -> Result<Index>>,
        app_control_tx: Sender<ControlMessage>,
    ) -> Result<()> {
        Ok(())
    }
    fn tick(&mut self, dt: f64, knob_state: LaserState) -> Result<()> {
        Ok(())
    }
    fn on_event(&mut self, event: &mut Event<()>) {}
    fn on_button_pressed(&mut self, button: UscButton) {}
    fn on_button_released(&mut self, button: UscButton) {}
    fn render(
        &mut self,
        dt: f64,
        td_context: &three_d::Context,
        target: &mut RenderTarget,
        viewport: Viewport,
    ) {
    }
    fn render_ui(&mut self, dt: f64) -> Result<()>;
    fn suspend(&mut self) {}
    fn resume(&mut self) {}
    fn is_suspended(&self) -> bool;
    fn debug_ui(&mut self, ctx: &three_d::egui::Context) -> Result<()>;
    fn closed(&self) -> bool;
    fn name(&self) -> &str;
}

pub trait SceneData
where
    Self: Send,
{
    fn make_scene(self: Box<Self>) -> Box<dyn Scene>;
}

impl SceneData for dyn Scene + Send {
    fn make_scene(self: Box<Self>) -> Box<dyn Scene> {
        self
    }
}

use std::{
    rc::Rc,
    sync::{mpsc::Sender, Arc, Mutex},
    time::SystemTime,
};

use anyhow::Result;
use di::ServiceProvider;
use game_loop::winit::event::Event;
use generational_arena::Index;
use rodio::dynamic_mixer::DynamicMixerController;
use tealr::mlu::mlua::Lua;
use three_d::{RenderTarget, Viewport};

use crate::{
    button_codes::{LaserState, UscButton, UscInputEvent},
    game_data::GameData,
    input_state::InputState,
    vg_ui::Vgfx,
    ControlMessage,
};

#[allow(unused_variables)]
pub trait Scene {
    fn init(&mut self, app_control_tx: Sender<ControlMessage>) -> Result<()> {
        Ok(())
    }
    fn tick(&mut self, dt: f64, knob_state: LaserState) -> Result<()> {
        Ok(())
    }
    fn on_event(&mut self, event: &Event<UscInputEvent>) {}
    fn on_button_pressed(&mut self, button: UscButton, timestamp: SystemTime) {}
    fn on_button_released(&mut self, button: UscButton, timestamp: SystemTime) {}
    fn render(
        &mut self,
        dt: f64,
        td_context: &three_d::Context,
        target: &mut RenderTarget,
        viewport: Viewport,
    ) {
    }
    fn render_ui(&mut self, dt: f64) -> Result<()>;
    fn has_egui(&self) -> bool {
        false
    }
    fn render_egui(&mut self, ctx: &egui::Context) -> Result<()> {
        Ok(())
    }
    fn suspend(&mut self) {}
    fn resume(&mut self) {}
    fn is_suspended(&self) -> bool;
    fn debug_ui(&mut self, ctx: &egui::Context) -> Result<()>;
    fn closed(&self) -> bool;
    fn name(&self) -> &str;
}

pub trait SceneData: Send {
    fn make_scene(
        self: Box<Self>,
        service_provider: ServiceProvider,
    ) -> anyhow::Result<Box<dyn Scene>>;
}

impl SceneData for dyn Scene + Send {
    fn make_scene(self: Box<Self>, _: ServiceProvider) -> anyhow::Result<Box<dyn Scene>> {
        Ok(self)
    }
}

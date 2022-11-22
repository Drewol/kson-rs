use std::{path::Path, rc::Rc, sync::mpsc::Sender};

use anyhow::Result;
use tealr::mlu::mlua::Lua;
use three_d::Event;

use crate::{
    button_codes::{LaserState, UscButton},
    game_data::GameData,
    ControlMessage,
};

#[allow(unused_variables)]
pub trait Scene {
    fn init(
        &mut self,
        load_lua: Box<dyn Fn(Rc<Lua>, &'static str) -> Result<()>>,
        app_control_tx: Sender<ControlMessage>,
    ) -> Result<()> {
        Ok(())
    }
    fn tick(&mut self, dt: f64, knob_state: LaserState) -> Result<bool> {
        Ok(false)
    }
    fn on_event(&mut self, event: &mut Event) {}
    fn on_button_pressed(&mut self, button: UscButton) {}
    fn on_button_released(&mut self, button: UscButton) {}
    fn render(&mut self, dt: f64) -> Result<bool>;
    fn suspend(&mut self) {}
    fn resume(&mut self) {}
    fn is_suspended(&self) -> bool;
    fn debug_ui(&mut self, ctx: &three_d::egui::Context) -> Result<()>;
}

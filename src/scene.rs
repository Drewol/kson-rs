use std::path::Path;

use anyhow::Result;
use tealr::mlu::mlua::Lua;
use three_d::Event;

use crate::game_data::GameData;

#[allow(unused_variables)]
pub trait Scene {
    fn init(&mut self, load_lua: Box<dyn Fn(&Lua, &'static str) -> Result<()>>) -> Result<()> {
        Ok(())
    }
    fn tick(&mut self, dt: f64, game_data: GameData) -> Result<bool> {
        Ok(false)
    }
    fn on_event(&mut self, event: &mut Event) {}
    fn render(&mut self, dt: f64) -> Result<bool>;
    fn suspend(&mut self) {}
    fn resume(&mut self) {}
    fn is_suspended(&self) -> bool;
    fn debug_ui(&mut self, ctx: &three_d::egui::Context) -> Result<()>;
}

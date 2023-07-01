use crate::{
    action_stack::ActionStack,
    chart_editor::{MainState, ScreenState},
    Modifiers,
};
use anyhow::Result;
use eframe::egui::Pos2;
use eframe::egui::{Context, Painter};
use kson::Chart;

mod bpm_ts;
mod buttons;
mod camera;
mod laser;
pub use bpm_ts::*;
pub use buttons::*;
pub use camera::*;
pub use laser::*;

pub trait CursorObject {
    fn primary_click(
        &mut self,
        _screen: ScreenState,
        _tick: u32,
        _tick_f: f64,
        _lane: f32,
        _chart: &Chart,
        _actions: &mut ActionStack<Chart>,
        _pos: Pos2,
    ) {
    }

    fn secondary_click(
        &mut self,
        _screen: ScreenState,
        _tick: u32,
        _tick_f: f64,
        _lane: f32,
        _chart: &Chart,
        _actions: &mut ActionStack<Chart>,
        _pos: Pos2,
    ) {
    }

    //Used as delete for most tools
    fn middle_click(
        &mut self,
        _screen: ScreenState,
        _tick: u32,
        _tick_f: f64,
        _lane: f32,
        _chart: &Chart,
        _actions: &mut ActionStack<Chart>,
        _pos: Pos2,
    ) {
    }

    fn drag_end(
        &mut self,
        _screen: ScreenState,
        _tick: u32,
        _tick_f: f64,
        _lane: f32,
        _chart: &Chart,
        _actions: &mut ActionStack<Chart>,
        _pos: Pos2,
    ) {
    }

    fn drag_start(
        &mut self,
        _screen: ScreenState,
        _tick: u32,
        _tick_f: f64,
        _lane: f32,
        _chart: &Chart,
        _actions: &mut ActionStack<Chart>,
        _pos: Pos2,
        _modifiers: &Modifiers,
    ) {
    }

    fn update(&mut self, tick: u32, tick_f: f64, lane: f32, pos: Pos2, chart: &Chart);
    fn draw(&self, state: &MainState, painter: &Painter) -> Result<()>;
    fn draw_ui(&mut self, _state: &mut MainState, _ctx: &Context) {}
}

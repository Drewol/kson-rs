use crate::{
    action_stack::ActionStack,
    chart_editor::{MainState, ScreenState},
};
use anyhow::Result;
use eframe::egui::{Context, Painter};
use kson::Chart;
use na::Point2;
use nalgebra as na;

mod bpm_ts;
mod buttons;
mod laser;
pub use bpm_ts::*;
pub use buttons::*;
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
        _pos: Point2<f32>,
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
        _pos: Point2<f32>,
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
        _pos: Point2<f32>,
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
        _pos: Point2<f32>,
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
        _pos: Point2<f32>,
    ) {
    }

    fn update(&mut self, tick: u32, tick_f: f64, lane: f32, pos: Point2<f32>);
    fn draw(&self, state: &MainState, painter: &Painter) -> Result<()>;
    fn draw_ui(&mut self, _ctx: &Context, _actions: &mut ActionStack<Chart>) {}
}

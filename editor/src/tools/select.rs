use eframe::egui::{Rgba, Shape};
use kson::{overlaps::Overlaps, rand::RngExt, Interval};
use log::info;

use crate::rect_xy_wh;

pub struct RangeSelect {
    selected: Interval,
    start_tick: u32,
}

impl Default for RangeSelect {
    fn default() -> Self {
        Self {
            selected: Interval { y: 0, l: 0 },
            start_tick: u32::MAX,
        }
    }
}

impl super::CursorObject for RangeSelect {
    fn primary_click(
        &mut self,
        _screen: crate::chart_editor::ScreenState,
        tick: u32,
        _tick_f: f64,
        _lane: f32,
        chart: &kson::Chart,
        actions: &mut crate::action_stack::ActionStack<kson::Chart>,
        _pos: emath::Pos2,
        modifiers: eframe::egui::Modifiers,
    ) {
        if !self.selected.contains(tick) {
            return;
        }

        let (start, end) = (self.selected.y, (self.selected.y + self.selected.l));

        // TODO: Show toast why the check fails
        if modifiers.command && chart.check_mirr_rand(start, end, true) {
            let seed = kson::rand::rng().random();
            actions.new_action("Randomize", move |chart| {
                chart
                    .randomize(start..=end, seed)
                    .map_err(anyhow::Error::from)
            });
        } else if modifiers.shift && chart.check_mirr_rand(start, end, false) {
            actions.new_action("Mirror", move |chart| {
                chart.mirror(start..=end).map_err(anyhow::Error::from)
            });
        }
    }

    fn drag_end(
        &mut self,
        _screen: crate::chart_editor::ScreenState,
        tick: u32,
        _tick_f: f64,
        _lane: f32,
        _chart: &kson::Chart,
        _actions: &mut crate::action_stack::ActionStack<kson::Chart>,
        _pos: emath::Pos2,
    ) {
        if tick > self.start_tick {
            self.selected.l = tick - self.start_tick;
            self.selected.y = self.start_tick;
        } else {
            self.selected.y = tick;
            self.selected.l = self.start_tick - tick;
        }

        self.start_tick = u32::MAX;
    }

    fn drag_start(
        &mut self,
        _screen: crate::chart_editor::ScreenState,
        tick: u32,
        _tick_f: f64,
        _lane: f32,
        _chart: &kson::Chart,
        _actions: &mut crate::action_stack::ActionStack<kson::Chart>,
        _pos: emath::Pos2,
        _modifiers: &crate::Modifiers,
    ) {
        self.selected.l = 0;
        self.selected.y = tick;
        self.start_tick = tick;
    }

    fn update(&mut self, tick: u32, tick_f: f64, lane: f32, pos: emath::Pos2, chart: &kson::Chart) {
        if self.start_tick == u32::MAX {
            return;
        }

        if tick > self.start_tick {
            self.selected.l = tick - self.start_tick;
            self.selected.y = self.start_tick;
        } else {
            self.selected.y = tick;
            self.selected.l = self.start_tick - tick;
        }
    }

    fn draw(
        &self,
        state: &crate::chart_editor::MainState,
        painter: &eframe::egui::Painter,
    ) -> anyhow::Result<()> {
        let color = Rgba::from_rgba_unmultiplied(1.0, 0.5, 0.0, 0.1);
        for (x, y, h, _) in state.screen.interval_to_ranges(&self.selected) {
            painter.add(Shape::rect_filled(
                rect_xy_wh([
                    x + state.screen.track_width / 2.0,
                    y,
                    state.screen.track_width,
                    h,
                ]),
                2.0,
                color,
            ));
        }
        Ok(())
    }
}

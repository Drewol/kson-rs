use crate::tools::CursorObject;
use crate::{
    action_stack::ActionStack,
    chart_editor::{MainState, ScreenState},
};
use anyhow::{bail, Result};
use eframe::egui::{self, Color32, CtxRef, DragValue, Label, Painter, Window};
use kson::Chart;
use na::Point2;
use nalgebra as na;

enum CursorToolStates {
    None,
    Add(u32),
    Edit(usize),
}

pub struct BpmTool {
    bpm: f64,
    state: CursorToolStates,
    cursor_tick: u32,
}

impl BpmTool {
    pub fn new() -> Self {
        BpmTool {
            bpm: 120.0,
            state: CursorToolStates::None,
            cursor_tick: 0,
        }
    }
}

impl CursorObject for BpmTool {
    fn primary_click(
        &mut self,
        _screen: ScreenState,
        tick: u32,
        _tick_f: f64,
        _lane: f32,
        chart: &Chart,
        _actions: &mut ActionStack<Chart>,
        _pos: Point2<f32>,
    ) {
        if let CursorToolStates::None = self.state {
            //check for bpm changes on selected tick
            for (i, change) in chart.beat.bpm.iter().enumerate() {
                if change.y == tick {
                    self.state = CursorToolStates::Edit(i);
                    self.bpm = change.v;
                    return;
                }
            }

            self.state = CursorToolStates::Add(tick);
        }
    }

    fn update(&mut self, tick: u32, _tick_f: f64, _lane: f32, _pos: Point2<f32>) {
        if let CursorToolStates::None = self.state {
            self.cursor_tick = tick;
        }
    }

    fn draw(&self, state: &MainState, painter: &Painter) -> Result<()> {
        state.draw_cursor_line(painter, self.cursor_tick, Color32::from_rgb(0, 128, 255));
        Ok(())
    }

    fn draw_ui(&mut self, ctx: &CtxRef, actions: &mut ActionStack<Chart>) {
        let complete_func: Option<Box<dyn Fn(&mut ActionStack<Chart>, f64)>> = match self.state {
            CursorToolStates::None => None,
            CursorToolStates::Add(tick) => {
                Some(Box::new(move |a: &mut ActionStack<Chart>, bpm: f64| {
                    let v = bpm;
                    let y = tick;

                    let new_action = a.new_action();

                    new_action.description = String::from("Add BPM Change");
                    new_action.action = Box::new(move |c| {
                        c.beat.bpm.push(kson::ByPulse { y, v });
                        c.beat.bpm.sort_by(|a, b| a.y.cmp(&b.y));
                        Ok(())
                    });
                }))
            }
            CursorToolStates::Edit(index) => {
                Some(Box::new(move |a: &mut ActionStack<Chart>, bpm: f64| {
                    let v = bpm;

                    let new_action = a.new_action();
                    new_action.description = String::from("Edit BPM Change");
                    new_action.action = Box::new(move |c| {
                        if let Some(change) = c.beat.bpm.get_mut(index) {
                            change.v = v;
                            Ok(())
                        } else {
                            bail!("Tried to edit non existing BPM Change")
                        }
                    });
                }))
            }
        };

        if let Some(complete) = complete_func {
            let mut bpm = self.bpm as f32;
            Window::new("Change BPM")
                .title_bar(true)
                .default_size([300.0, 600.0])
                .default_pos([100.0, 100.0])
                .show(ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.add(Label::new("BPM:"));
                        ui.add(DragValue::new(&mut bpm).speed(0.1));
                        self.bpm = bpm as f64;

                        ui.end_row();
                        ui.end_row();

                        if ui.button("Cancel").clicked() {
                            self.state = CursorToolStates::None;
                        }
                        if ui.button("Ok").clicked() {
                            complete(actions, bpm as f64);
                            self.state = CursorToolStates::None;
                        }
                    });
                });
        }
    }
}

pub struct TimeSigTool {
    ts: kson::TimeSignature,
    state: CursorToolStates,
    cursor_tick: u32,
}

impl TimeSigTool {
    pub fn new() -> Self {
        TimeSigTool {
            ts: kson::TimeSignature { d: 4, n: 4 },
            state: CursorToolStates::None,
            cursor_tick: 0,
        }
    }
}

impl CursorObject for TimeSigTool {
    fn primary_click(
        &mut self,
        _screen: ScreenState,
        tick: u32,
        _tick_f: f64,
        _lane: f32,
        chart: &Chart,
        _actions: &mut ActionStack<Chart>,
        _pos: Point2<f32>,
    ) {
        let measure = chart.tick_to_measure(tick);
        if let CursorToolStates::None = self.state {
            //check for bpm changes on selected tick
            if let Ok(idx) = chart
                .beat
                .time_sig
                .binary_search_by(|tsc| tsc.idx.cmp(&measure))
            {
                self.state = CursorToolStates::Edit(idx);
                self.ts = chart.beat.time_sig.get(idx).unwrap().v;
            } else {
                self.state = CursorToolStates::Add(measure);
                self.ts = kson::TimeSignature { d: 4, n: 4 };
            }
        }
    }

    fn update(&mut self, tick: u32, _tick_f: f64, _lane: f32, _pos: Point2<f32>) {
        if let CursorToolStates::None = self.state {
            self.cursor_tick = tick;
        }
    }

    fn draw(&self, state: &MainState, painter: &Painter) -> Result<()> {
        let tick = state
            .chart
            .measure_to_tick(state.chart.tick_to_measure(self.cursor_tick));
        state.draw_cursor_line(painter, tick, Color32::from_rgb(255, 255, 0));
        Ok(())
    }

    fn draw_ui(&mut self, ctx: &CtxRef, actions: &mut ActionStack<Chart>) {
        let complete_func: Option<Box<dyn Fn(&mut ActionStack<Chart>, [i32; 2])>> = match self.state
        {
            CursorToolStates::None => None,
            CursorToolStates::Add(measure) => Some(Box::new(move |a, ts| {
                let v = kson::TimeSignature {
                    n: ts[0] as u32,
                    d: ts[1] as u32,
                };
                let idx = measure;

                let new_action = a.new_action();
                new_action.description = String::from("Add Time Signature Change");
                new_action.action = Box::new(move |c| {
                    c.beat.time_sig.push(kson::ByMeasureIndex { idx, v });
                    c.beat.time_sig.sort_by(|a, b| a.idx.cmp(&b.idx));
                    Ok(())
                });
            })),
            CursorToolStates::Edit(index) => Some(Box::new(move |a, ts| {
                let new_action = a.new_action();
                new_action.description = String::from("Edit Time Signature Change");
                new_action.action = Box::new(move |c| {
                    if let Some(change) = c.beat.time_sig.get_mut(index) {
                        change.v.n = ts[0] as u32;
                        change.v.d = ts[1] as u32;
                        Ok(())
                    } else {
                        bail!("Tried to edit non existing Time Signature Change")
                    }
                });
            })),
        };

        if let Some(complete) = complete_func {
            egui::Window::new("Change Time Signature")
                .title_bar(true)
                .default_size([300.0, 600.0])
                .default_pos([100.0, 100.0])
                .show(ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let (mut ts_n, mut ts_d) = (self.ts.n, self.ts.d);

                        ui.add(egui::widgets::DragValue::new(&mut ts_n).speed(0.2));
                        ui.add(egui::Label::new("/"));
                        ui.add(egui::widgets::DragValue::new(&mut ts_d).speed(0.2));
                        ui.end_row();
                        ui.end_row();

                        self.ts.n = ts_n;
                        self.ts.d = ts_d;

                        if ui.button("Ok").clicked() {
                            complete(actions, [ts_n as i32, ts_d as i32]);
                            self.state = CursorToolStates::None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.state = CursorToolStates::None;
                        }
                    });
                });
        }
    }
}

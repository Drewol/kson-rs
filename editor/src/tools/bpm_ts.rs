use crate::i18n;
use crate::tools::CursorObject;
use crate::{
    action_stack::ActionStack,
    chart_editor::{MainState, ScreenState},
};
use anyhow::{bail, Result};
use eframe::egui::{self, Color32, Context, DragValue, Label, Painter, Pos2, Window};
use kson::Chart;
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
    pub const fn new() -> Self {
        BpmTool {
            bpm: 120.0,
            state: CursorToolStates::None,
            cursor_tick: 0,
        }
    }
}

type CompletionFn<T> = Box<dyn Fn(&mut ActionStack<Chart>, T)>;

impl CursorObject for BpmTool {
    fn primary_click(
        &mut self,
        _screen: ScreenState,
        tick: u32,
        _tick_f: f64,
        _lane: f32,
        chart: &Chart,
        _actions: &mut ActionStack<Chart>,
        _pos: Pos2,
    ) {
        if let CursorToolStates::None = self.state {
            //check for bpm changes on selected tick
            for (i, change) in chart.beat.bpm.iter().enumerate() {
                if change.0 == tick {
                    self.state = CursorToolStates::Edit(i);
                    self.bpm = change.1;
                    return;
                }
            }

            self.state = CursorToolStates::Add(tick);
        }
    }

    fn update(&mut self, tick: u32, _tick_f: f64, _lane: f32, _pos: Pos2, _chart: &Chart) {
        if let CursorToolStates::None = self.state {
            self.cursor_tick = tick;
        }
    }

    fn draw(&self, state: &MainState, painter: &Painter) -> Result<()> {
        state.draw_cursor_line(painter, self.cursor_tick, Color32::from_rgb(0, 128, 255));
        Ok(())
    }

    fn draw_ui(&mut self, state: &mut MainState, ctx: &Context) {
        let complete_func: Option<CompletionFn<f64>> = match self.state {
            CursorToolStates::None => None,
            CursorToolStates::Add(tick) => {
                Some(Box::new(move |a: &mut ActionStack<Chart>, bpm: f64| {
                    let v = bpm;
                    let y = tick;

                    a.new_action(i18n::fl!("add_bpm_change"), move |c| {
                        c.beat.bpm.push((y, v));
                        c.beat.bpm.sort_by(|a, b| a.0.cmp(&b.0));
                        Ok(())
                    });
                }))
            }
            CursorToolStates::Edit(index) => {
                Some(Box::new(move |a: &mut ActionStack<Chart>, bpm: f64| {
                    let v = bpm;

                    a.new_action(i18n::fl!("edit_bpm_change"), move |c| {
                        if let Some(change) = c.beat.bpm.get_mut(index) {
                            change.1 = v;
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
            Window::new(i18n::fl!("change_bpm"))
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

                        if ui.button(i18n::fl!("cancel")).clicked() {
                            self.state = CursorToolStates::None;
                        }
                        if ui.button(i18n::fl!("ok")).clicked() {
                            complete(&mut state.actions, bpm as f64);
                            self.state = CursorToolStates::None;
                        }
                    });
                });
        }
    }

    fn middle_click(
        &mut self,
        _screen: ScreenState,
        tick: u32,
        _tick_f: f64,
        _lane: f32,
        chart: &Chart,
        actions: &mut ActionStack<Chart>,
        _pos: Pos2,
    ) {
        if let Ok(index) = chart.beat.bpm.binary_search_by_key(&tick, |f| f.0) {
            actions.new_action(i18n::fl!("remove_bpm_change"), move |chart: &mut Chart| {
                chart.beat.bpm.remove(index);
                Ok(())
            })
        }
    }
}

pub struct TimeSigTool {
    ts: kson::TimeSignature,
    state: CursorToolStates,
    cursor_tick: u32,
}

impl TimeSigTool {
    pub const fn new() -> Self {
        TimeSigTool {
            ts: kson::TimeSignature(4, 4),
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
        _pos: Pos2,
    ) {
        let measure = chart.tick_to_measure(tick);
        if let CursorToolStates::None = self.state {
            //check for bpm changes on selected tick
            if let Ok(idx) = chart
                .beat
                .time_sig
                .binary_search_by(|tsc| tsc.0.cmp(&measure))
            {
                self.state = CursorToolStates::Edit(idx);
                self.ts = chart.beat.time_sig[idx].1;
            } else {
                self.state = CursorToolStates::Add(measure);
                self.ts = kson::TimeSignature(4, 4);
            }
        }
    }

    fn middle_click(
        &mut self,
        _screen: ScreenState,
        tick: u32,
        _tick_f: f64,
        _lane: f32,
        chart: &Chart,
        actions: &mut ActionStack<Chart>,
        _pos: Pos2,
    ) {
        let measure = chart.tick_to_measure(tick);
        if let Ok(index) = chart.beat.time_sig.binary_search_by_key(&measure, |f| f.0) {
            actions.new_action(
                i18n::fl!("remove_time_signature_change"),
                move |chart: &mut Chart| {
                    chart.beat.time_sig.remove(index);
                    Ok(())
                },
            )
        }
    }

    fn update(&mut self, tick: u32, _tick_f: f64, _lane: f32, _pos: Pos2, _chart: &Chart) {
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

    fn draw_ui(&mut self, state: &mut MainState, ctx: &Context) {
        let complete_func: Option<CompletionFn<[i32; 2]>> = match self.state {
            CursorToolStates::None => None,
            CursorToolStates::Add(measure) => Some(Box::new(move |a, ts| {
                let v = kson::TimeSignature(ts[0] as u32, ts[1] as u32);
                let idx = measure;

                a.new_action(i18n::fl!("add_time_signature_change"), move |c| {
                    c.beat.time_sig.push((idx, v));
                    c.beat.time_sig.sort_by(|a, b| a.0.cmp(&b.0));
                    Ok(())
                });
            })),
            CursorToolStates::Edit(index) => Some(Box::new(move |a, ts| {
                a.new_action(i18n::fl!("edit_time_signature_change"), move |c| {
                    if let Some(change) = c.beat.time_sig.get_mut(index) {
                        change.1 .0 = ts[0] as u32;
                        change.1 .1 = ts[1] as u32;
                        Ok(())
                    } else {
                        bail!("Tried to edit non existing Time Signature Change")
                    }
                });
            })),
        };

        if let Some(complete) = complete_func {
            egui::Window::new(i18n::fl!("change_time_signature"))
                .title_bar(true)
                .default_size([300.0, 600.0])
                .default_pos([100.0, 100.0])
                .show(ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let (mut ts_n, mut ts_d) = (self.ts.0, self.ts.1);

                        ui.add(egui::widgets::DragValue::new(&mut ts_n).speed(0.2));
                        ui.add(egui::Label::new("/"));
                        ui.add(egui::widgets::DragValue::new(&mut ts_d).speed(0.2));
                        ui.end_row();
                        ui.end_row();

                        self.ts.0 = ts_n;
                        self.ts.1 = ts_d;

                        if ui.button(i18n::fl!("ok")).clicked() {
                            complete(&mut state.actions, [ts_n as i32, ts_d as i32]);
                            self.state = CursorToolStates::None;
                        }
                        if ui.button(i18n::fl!("cancel")).clicked() {
                            self.state = CursorToolStates::None;
                        }
                    });
                });
        }
    }
}

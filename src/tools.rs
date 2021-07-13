use crate::{
    action_stack::{Action, ActionStack},
    chart_editor::{MainState, ScreenState},
    rect_xy_wh,
};
use anyhow::Result;
use eframe::egui::{self, CtxRef, DragValue, Label, Pos2, Rgba, Stroke, Window};
use eframe::egui::{Painter, Shape};
use na::point;
use na::Point2;
use nalgebra as na;

use kson::{Chart, GraphSectionPoint, Interval, LaserSection};

pub trait CursorObject {
    fn mouse_down(
        &mut self,
        screen: ScreenState,
        tick: u32,
        tick_f: f64,
        lane: f32,
        chart: &Chart,
        actions: &mut ActionStack<Chart>,
        pos: Point2<f32>,
    );
    fn mouse_up(
        &mut self,
        screen: ScreenState,
        tick: u32,
        tick_f: f64,
        lane: f32,
        chart: &Chart,
        actions: &mut ActionStack<Chart>,
        pos: Point2<f32>,
    );
    fn update(&mut self, tick: u32, tick_f: f64, lane: f32, pos: Point2<f32>);
    fn draw(&self, state: &MainState, painter: &Painter) -> Result<()>;
    fn draw_ui(&mut self, ctx: &CtxRef, actions: &mut ActionStack<Chart>);
}

//structs for cursor objects
pub struct ButtonInterval {
    pressed: bool,
    fx: bool,
    interval: Interval,
    lane: usize,
}

impl ButtonInterval {
    pub fn new(fx: bool) -> Self {
        ButtonInterval {
            pressed: false,
            fx,
            interval: Interval { y: 0, l: 0 },
            lane: 0,
        }
    }
}
#[derive(Copy, Clone)]
struct LaserEditState {
    section_index: usize,
    curving_index: Option<usize>,
}

enum LaserEditMode {
    None,
    New,
    Edit(LaserEditState),
}

pub struct LaserTool {
    right: bool,
    section: LaserSection,
    mode: LaserEditMode,
}

impl LaserTool {
    pub fn new(right: bool) -> Self {
        LaserTool {
            right,
            mode: LaserEditMode::None,
            section: LaserSection {
                y: 0,
                wide: 0,
                v: Vec::new(),
            },
        }
    }

    fn gsp(ry: u32, v: f64) -> GraphSectionPoint {
        GraphSectionPoint {
            ry,
            v,
            vf: None,
            a: Some(0.5),
            b: Some(0.5),
        }
    }

    fn get_control_point_pos(
        screen: ScreenState,
        points: &[GraphSectionPoint],
        start_y: u32,
    ) -> Option<Pos2> {
        let start = points.get(0).unwrap();
        //TODO: (a,b) should not be optional
        if start.a == None || start.b == None {
            return None;
        }
        let start_value = if let Some(vf) = start.vf { vf } else { start.v };
        let end = points.get(1).unwrap();
        let start_tick = start_y + start.ry;
        let end_tick = start_y + end.ry;
        match start_tick.cmp(&end_tick) {
            std::cmp::Ordering::Greater => panic!("Laser section start later than end."),
            std::cmp::Ordering::Equal => return None,
            _ => {}
        };
        let intervals = screen.interval_to_ranges(&Interval {
            y: start_tick,
            l: end_tick - start_tick,
        });

        if let Some(&interv) = intervals.iter().find(|&&v| {
            let a = start.a.unwrap();
            let s = (v.3).0 as f64;
            let e = (v.3).1 as f64;
            a >= s && a <= e
        }) {
            let value_width = end.v - start_value;
            let x = (start_value + start.b.unwrap() * value_width) as f32;
            let x = 1.0 / 10.0 + x * 8.0 / 10.0;
            let x = x * screen.track_width + interv.0 + screen.track_width / 2.0;
            let y = interv.1 + interv.2 * (start.a.unwrap() as f32 - (interv.3).0) / (interv.3).1;
            Some(Pos2::new(x - screen.x_offset, y))
        } else {
            panic!("Curve `a` was not in any interval");
        }
    }

    fn lane_to_pos(lane: f32) -> f64 {
        let resolution: f64 = 10.0;
        math::round::floor(resolution * lane as f64 / 6.0, 0) / resolution
    }

    fn get_second_to_last(&self) -> Option<&GraphSectionPoint> {
        let len = self.section.v.len();
        let idx = len.checked_sub(2);
        idx.and_then(|i| self.section.v.get(i))
    }

    /*
    fn get_second_to_last_mut(&mut self) -> Option<&mut GraphSectionPoint> {
        let len = self.section.v.len();
        let idx = len.checked_sub(2);
        let idx = idx.unwrap();
        self.section.v.get_mut(idx)
    }
    */

    fn calc_ry(&self, tick: u32) -> u32 {
        let ry = if tick <= self.section.y {
            0
        } else {
            tick - self.section.y
        };

        if let Some(secont_last) = self.get_second_to_last() {
            (*secont_last).ry.max(ry)
        } else {
            ry
        }
    }

    fn hit_test(&self, chart: &Chart, tick: u32) -> Option<usize> {
        let side_index: usize = if self.right { 1 } else { 0 };

        for si in 0..chart.note.laser[side_index].len() {
            let current_section = &chart.note.laser[side_index][si];
            if tick < current_section.y {
                break;
            }
            if tick >= current_section.y
                && tick <= current_section.y + current_section.v.last().unwrap().ry
            {
                return Some(si);
            }
        }
        None
    }
}

impl CursorObject for ButtonInterval {
    fn mouse_down(
        &mut self,
        _screen: ScreenState,
        tick: u32,
        _tick_f: f64,
        lane: f32,
        _chart: &Chart,
        _actions: &mut ActionStack<Chart>,
        _pos: Point2<f32>,
    ) {
        self.pressed = true;
        if self.fx {
            self.lane = if lane < 3.0 { 0 } else { 1 };
        } else {
            self.lane = (lane as usize).max(1).min(4) - 1;
        }
        self.interval.y = tick;
    }

    fn mouse_up(
        &mut self,
        _screen: ScreenState,
        tick: u32,
        _tick_f: f64,
        _lane: f32,
        _chart: &Chart,
        actions: &mut ActionStack<Chart>,
        _pos: Point2<f32>,
    ) {
        if self.interval.y >= tick {
            self.interval.l = 0;
        } else {
            self.interval.l = tick - self.interval.y;
        }
        let v = std::mem::replace(&mut self.interval, Interval { y: 0, l: 0 });
        if self.fx {
            let l = self.lane;

            actions.commit(Action {
                description: format!(
                    "Add {} FX Note",
                    if self.lane == 0 { "Left" } else { "Right" }
                ),
                action: Box::new(move |edit_chart: &mut Chart| {
                    edit_chart.note.fx[l].push(v);
                    edit_chart.note.fx[l].sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
                    Ok(())
                }),
            })
        } else {
            let l = self.lane;

            actions.commit(Action {
                description: format!(
                    "Add {} BT Note",
                    std::char::from_u32('A' as u32 + self.lane as u32).unwrap_or_default()
                ),
                action: Box::new(move |edit_chart: &mut Chart| {
                    edit_chart.note.bt[l].push(v);
                    edit_chart.note.bt[l].sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
                    Ok(())
                }),
            })
        }
        self.pressed = false;
        self.lane = 0;
    }

    fn update(&mut self, tick: u32, _tick_f: f64, lane: f32, _pos: Point2<f32>) {
        if !self.pressed {
            self.interval.y = tick;
            if self.fx {
                self.lane = if lane < 3.0 { 0 } else { 1 };
            } else {
                self.lane = (lane as usize).max(1).min(4) - 1;
            }
        }
        if self.interval.y >= tick {
            self.interval.l = 0;
        } else {
            self.interval.l = tick - self.interval.y;
        }
    }

    fn draw(&self, state: &MainState, painter: &Painter) -> Result<()> {
        let color = if self.fx {
            Rgba::from_rgba_premultiplied(1.0, 0.3, 0.0, 0.5)
        } else {
            Rgba::from_rgba_premultiplied(1.0, 1.0, 1.0, 0.5)
        };
        if self.interval.l == 0 {
            let (x, y) = state.screen.tick_to_pos(self.interval.y);

            let x = if self.fx {
                x + self.lane as f32 * state.screen.lane_width() * 2.0
                    + 2.0 * self.lane as f32
                    + state.screen.lane_width()
                    + state.screen.track_width / 2.0
            } else {
                x + self.lane as f32 * state.screen.lane_width()
                    + 1.0 * self.lane as f32
                    + state.screen.lane_width()
                    + state.screen.track_width / 2.0
            };
            let y = y as f32;

            let w = if self.fx {
                state.screen.track_width as f32 / 3.0 - 1.0
            } else {
                state.screen.track_width as f32 / 6.0 - 2.0
            };
            let h = -2.0;

            painter.rect_filled(rect_xy_wh([x, y, w, h]), 0.0, color);
            Ok(())
        } else {
            let mut long_bt_builder = Vec::<Shape>::new();
            for (x, y, h, _) in state.screen.interval_to_ranges(&self.interval) {
                let x = if self.fx {
                    x + self.lane as f32 * state.screen.lane_width() * 2.0
                        + 2.0 * self.lane as f32
                        + state.screen.lane_width()
                        + state.screen.track_width / 2.0
                } else {
                    x + self.lane as f32 * state.screen.lane_width()
                        + 1.0 * self.lane as f32
                        + state.screen.lane_width()
                        + state.screen.track_width / 2.0
                };

                let w = if self.fx {
                    state.screen.track_width as f32 / 3.0 - 1.0
                } else {
                    state.screen.track_width as f32 / 6.0 - 2.0
                };

                long_bt_builder.push(Shape::rect_filled(rect_xy_wh([x, y, w, h]), 0.0, color));
            }

            painter.extend(long_bt_builder);
            Ok(())
        }
    }

    fn draw_ui(&mut self, _ctx: &CtxRef, _actions: &mut ActionStack<Chart>) {}
}

impl CursorObject for LaserTool {
    fn mouse_down(
        &mut self,
        screen: ScreenState,
        tick: u32,
        _tick_f: f64,
        lane: f32,
        chart: &Chart,
        actions: &mut ActionStack<Chart>,
        pos: Point2<f32>,
    ) {
        let v = LaserTool::lane_to_pos(lane);
        let ry = self.calc_ry(tick);
        let mut finalize = false;

        match self.mode {
            LaserEditMode::None => {
                //hit test existing lasers
                //if a laser exists enter edit mode for that laser
                //if no lasers exist create new laser
                let side_index: usize = if self.right { 1 } else { 0 };
                if let Some(section_index) = self.hit_test(chart, tick) {
                    self.section = chart.note.laser[side_index][section_index].clone();
                    self.mode = LaserEditMode::Edit(LaserEditState {
                        section_index,
                        curving_index: None,
                    });
                } else {
                    self.section.y = tick;
                    self.section.v.push(LaserTool::gsp(0, v));
                    self.section.v.push(LaserTool::gsp(0, v));
                    self.section.wide = 1;
                    self.mode = LaserEditMode::New;
                }
            }
            LaserEditMode::New => {
                if let Some(last) = self.get_second_to_last() {
                    finalize = match (*last).vf {
                        Some(_) => ry == last.ry,
                        None => ry == last.ry && (v - last.v).abs() < f64::EPSILON,
                    };
                }
                if finalize {
                    self.mode = LaserEditMode::None;
                    self.section.v.pop();
                    let v = std::mem::replace(
                        &mut self.section,
                        LaserSection {
                            y: 0,
                            v: Vec::new(),
                            wide: 1,
                        },
                    );
                    let v = std::rc::Rc::new(v.clone()); //Can't capture by clone so use RC
                    let i = if self.right { 1 } else { 0 };
                    actions.commit(Action {
                        description: format!(
                            "Add {} Laser",
                            if self.right { "Right" } else { "Left" }
                        ),
                        action: Box::new(move |edit_chart| {
                            edit_chart.note.laser[i].push(v.as_ref().clone());
                            edit_chart.note.laser[i].sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
                            Ok(())
                        }),
                    });

                    return;
                }

                self.section
                    .v
                    .push(LaserTool::gsp(ry, LaserTool::lane_to_pos(lane)));
            }
            LaserEditMode::Edit(edit_state) => {
                if self.hit_test(chart, tick) == Some(edit_state.section_index) {
                    for (i, points) in self.section.v.windows(2).enumerate() {
                        if let Some(control_point) =
                            LaserTool::get_control_point_pos(screen, points, self.section.y)
                        {
                            if na::distance(&point![control_point.x, control_point.y], &pos) < 5.0 {
                                self.mode = LaserEditMode::Edit(LaserEditState {
                                    section_index: edit_state.section_index,
                                    curving_index: Some(i),
                                })
                            }
                        }
                    }
                //TODO: Subdivide and stuff
                } else {
                    self.mode = LaserEditMode::None;
                    self.section = LaserSection {
                        y: tick,
                        v: Vec::new(),
                        wide: 1,
                    }
                }
            }
        }
    }
    fn mouse_up(
        &mut self,
        _screen: ScreenState,
        _tick: u32,
        _tick_f: f64,
        _lane: f32,
        _chart: &Chart,
        actions: &mut ActionStack<Chart>,
        _pos: Point2<f32>,
    ) {
        if let LaserEditMode::Edit(edit_state) = self.mode {
            if let Some(curving_index) = edit_state.curving_index {
                let right = self.right;
                let laser_text = if right { "Right" } else { "Left" };
                let section_index = edit_state.section_index;
                let laser_i = if right { 1 } else { 0 };
                let updated_point = self.section.v[curving_index];
                actions.commit(Action {
                    description: format!("Adjust {} Laser Curve", laser_text),
                    action: Box::new(move |c| {
                        c.note.laser[laser_i][section_index].v[curving_index] = updated_point;
                        Ok(())
                    }),
                });
            }
            self.mode = LaserEditMode::Edit(LaserEditState {
                section_index: edit_state.section_index,
                curving_index: None,
            })
        }
    }

    fn update(&mut self, tick: u32, tick_f: f64, lane: f32, pos: Point2<f32>) {
        match self.mode {
            LaserEditMode::New => {
                let ry = self.calc_ry(tick);
                let v = LaserTool::lane_to_pos(lane);
                let second_last: Option<GraphSectionPoint> = match self.get_second_to_last() {
                    Some(sl) => Some(*sl),
                    None => None,
                };
                if let Some(last) = self.section.v.last_mut() {
                    (*last).ry = ry;
                    (*last).v = v;

                    if let Some(second_last) = second_last {
                        if second_last.ry == ry {
                            (*last).v = second_last.v;
                            (*last).vf = Some(v);
                        } else {
                            (*last).vf = None;
                        }
                    }
                }
            }
            LaserEditMode::None => {}
            LaserEditMode::Edit(edit_state) => {
                for gp in &mut self.section.v {
                    if gp.a.is_none() {
                        gp.a = Some(0.5);
                    }
                    if gp.b.is_none() {
                        gp.b = Some(0.5);
                    }
                }
                if let Some(curving_index) = edit_state.curving_index {
                    let end_point = self.section.v[curving_index + 1];
                    let point = &mut self.section.v[curving_index];
                    let start_tick = (self.section.y + point.ry) as f64;
                    let end_tick = (self.section.y + end_point.ry) as f64;
                    point.a = Some(
                        ((tick_f - start_tick) / (end_tick - start_tick))
                            .max(0.0)
                            .min(1.0),
                    );

                    let start_value = point.vf.unwrap_or(point.v);
                    let in_value = lane as f64 / 6.0;
                    let value = (in_value - start_value) / (end_point.v - start_value);

                    self.section.v[curving_index].b = Some(value.min(1.0).max(0.0));
                }
            }
        }
    }
    fn draw(&self, state: &MainState, painter: &Painter) -> Result<()> {
        if self.section.v.len() > 1 {
            //Draw laser mesh
            if let Some(color) = match self.mode {
                LaserEditMode::None => None,
                LaserEditMode::New => {
                    let b = 0.8;
                    if self.right {
                        Some(Rgba::from_rgba_premultiplied(
                            0.76 * b,
                            0.024 * b,
                            0.55 * b,
                            1.0,
                        ))
                    } else {
                        Some(Rgba::from_rgba_premultiplied(0.0, 0.45 * b, 0.565 * b, 1.0))
                    }
                }
                LaserEditMode::Edit(_) => Some(Rgba::from_rgba_premultiplied(0.0, 0.76, 0.0, 1.0)),
            } {
                let mut mb = Vec::new();
                state.draw_laser_section(&self.section, &mut mb, color.into())?;
                painter.extend(mb);
            }

            //Draw curve control points
            if let LaserEditMode::Edit(edit_state) = self.mode {
                for (i, start_end) in self.section.v.windows(2).enumerate() {
                    let color = if edit_state.curving_index == Some(i) {
                        Rgba::from_rgba_premultiplied(0.0, 1.0, 0.0, 1.0)
                    } else {
                        Rgba::from_rgba_premultiplied(0.0, 0.0, 1.0, 1.0)
                    };

                    if let Some(pos) =
                        LaserTool::get_control_point_pos(state.screen, start_end, self.section.y)
                    {
                        painter.circle(pos, 5.0, color, Stroke::none());
                    }
                }
            }
        }
        Ok(())
    }
    fn draw_ui(&mut self, _ctx: &CtxRef, _actions: &mut ActionStack<Chart>) {}
}

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
    fn mouse_down(
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

    fn mouse_up(
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

    fn update(&mut self, tick: u32, _tick_f: f64, _lane: f32, _pos: Point2<f32>) {
        if let CursorToolStates::None = self.state {
            self.cursor_tick = tick;
        }
    }

    fn draw(&self, state: &MainState, painter: &Painter) -> Result<()> {
        state.draw_cursor_line(painter, self.cursor_tick, (0, 128, 255, 255));
        Ok(())
    }

    fn draw_ui(&mut self, ctx: &CtxRef, actions: &mut ActionStack<Chart>) {
        let complete_func: Option<Box<dyn Fn(&mut ActionStack<Chart>, f64) -> ()>> =
            match self.state {
                CursorToolStates::None => None,
                CursorToolStates::Add(tick) => {
                    Some(Box::new(move |a: &mut ActionStack<Chart>, bpm: f64| {
                        let v = bpm;
                        let y = tick;
                        a.commit(Action {
                            description: String::from("Add BPM Change"),
                            action: Box::new(move |c| {
                                c.beat.bpm.push(kson::ByPulse { v, y });
                                c.beat.bpm.sort_by(|a, b| a.y.cmp(&b.y));
                                Ok(())
                            }),
                        })
                    }))
                }
                CursorToolStates::Edit(index) => {
                    Some(Box::new(move |a: &mut ActionStack<Chart>, bpm: f64| {
                        let v = bpm;
                        a.commit(Action {
                            description: String::from("Edit BPM Change"),
                            action: Box::new(move |c| {
                                if let Some(change) = c.beat.bpm.get_mut(index) {
                                    change.v = v;
                                    Ok(())
                                } else {
                                    Err(String::from("Tried to edit non existing BPM Change"))
                                }
                            }),
                        })
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
                    ui.add(Label::new("BPM:"));
                    ui.add(DragValue::new(&mut bpm).speed(0.1));
                    self.bpm = bpm as f64;
                    ui.end_row();
                    if ui.button("Cancel").clicked() {
                        self.state = CursorToolStates::None;
                    }
                    if ui.button("Ok").clicked() {
                        complete(actions, bpm as f64);
                        self.state = CursorToolStates::None;
                    }
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
    fn mouse_down(
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

    fn mouse_up(
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

    fn update(&mut self, tick: u32, _tick_f: f64, _lane: f32, _pos: Point2<f32>) {
        if let CursorToolStates::None = self.state {
            self.cursor_tick = tick;
        }
    }

    fn draw(&self, state: &MainState, painter: &Painter) -> Result<()> {
        let tick = state
            .chart
            .measure_to_tick(state.chart.tick_to_measure(self.cursor_tick));
        state.draw_cursor_line(painter, tick, (255, 255, 0, 255));
        Ok(())
    }

    fn draw_ui(&mut self, ctx: &CtxRef, actions: &mut ActionStack<Chart>) {
        let complete_func: Option<Box<dyn Fn(&mut ActionStack<Chart>, [i32; 2]) -> ()>> =
            match self.state {
                CursorToolStates::None => None,
                CursorToolStates::Add(measure) => Some(Box::new(move |a, ts| {
                    let v = kson::TimeSignature {
                        n: ts[0] as u32,
                        d: ts[1] as u32,
                    };
                    let idx = measure;
                    a.commit(Action {
                        description: String::from("Add Time Signature Change"),
                        action: Box::new(move |c| {
                            c.beat.time_sig.push(kson::ByMeasureIndex { idx, v });
                            c.beat.time_sig.sort_by(|a, b| a.idx.cmp(&b.idx));
                            Ok(())
                        }),
                    })
                })),
                CursorToolStates::Edit(index) => Some(Box::new(move |a, ts| {
                    a.commit(Action {
                        description: String::from("Edit Time Signature Change"),
                        action: Box::new(move |c| {
                            if let Some(change) = c.beat.time_sig.get_mut(index) {
                                change.v.n = ts[0] as u32;
                                change.v.d = ts[1] as u32;
                                Ok(())
                            } else {
                                Err(String::from(
                                    "Tried to edit non existing Time Signature Change",
                                ))
                            }
                        }),
                    })
                })),
            };

        if let Some(complete) = complete_func {
            egui::Window::new("Change Time Signature")
                .title_bar(true)
                .default_size([300.0, 600.0])
                .default_pos([100.0, 100.0])
                .show(ctx, |ui| {
                    let (mut ts_n, mut ts_d) = (self.ts.n as f32, self.ts.d as f32);

                    ui.add(egui::widgets::DragValue::new(&mut ts_n).speed(1));
                    ui.add(egui::Label::new("/"));
                    ui.add(egui::widgets::DragValue::new(&mut ts_d).speed(1));
                    ui.end_row();

                    self.ts.n = ts_n.max(1.0) as u32;
                    self.ts.d = ts_d.max(1.0) as u32;

                    if ui.button("Ok").clicked() {
                        complete(actions, [ts_n as i32, ts_d as i32]);
                        self.state = CursorToolStates::None;
                    }
                    if ui.button("Cancel").clicked() {
                        self.state = CursorToolStates::None;
                    }
                });
        }
    }
}

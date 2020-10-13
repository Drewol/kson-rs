use crate::action_stack::{Action, ActionStack};
use crate::{MainState, ScreenState};

use ggez::graphics;
use ggez::nalgebra as na;
use ggez::{Context, GameResult};
use imgui::*;
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
        pos: na::Point2<f32>,
    );
    fn mouse_up(
        &mut self,
        screen: ScreenState,
        tick: u32,
        tick_f: f64,
        lane: f32,
        chart: &Chart,
        actions: &mut ActionStack<Chart>,
        pos: na::Point2<f32>,
    );
    fn update(&mut self, tick: u32, tick_f: f64, lane: f32, pos: na::Point2<f32>);
    fn draw(&self, state: &MainState, ctx: &mut Context) -> GameResult;
    fn draw_ui(&mut self, ui: &Ui, actions: &mut ActionStack<Chart>);
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
    ) -> Option<ggez::nalgebra::Point2<f32>> {
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
            return Some(ggez::nalgebra::Point2::new(x - screen.x_offset, y));
        } else {
            panic!("Curve `a` was not in any interval");
        }
        None
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
        screen: ScreenState,
        tick: u32,
        tick_f: f64,
        lane: f32,
        chart: &Chart,
        actions: &mut ActionStack<Chart>,
        pos: na::Point2<f32>,
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
        screen: ScreenState,
        tick: u32,
        tick_f: f64,
        _lane: f32,
        chart: &Chart,
        actions: &mut ActionStack<Chart>,
        pos: na::Point2<f32>,
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

    fn update(&mut self, tick: u32, tick_f: f64, lane: f32, pos: na::Point2<f32>) {
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

    fn draw(&self, state: &MainState, ctx: &mut Context) -> GameResult {
        graphics::set_blend_mode(ctx, graphics::BlendMode::Alpha)?;
        let color = if self.fx {
            graphics::Color {
                r: 1.0,
                g: 0.3,
                b: 0.0,
                a: 0.5,
            }
        } else {
            graphics::Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.5,
            }
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

            let m = graphics::Mesh::new_rectangle(
                ctx,
                graphics::DrawMode::fill(),
                [x, y, w, h].into(),
                color,
            )?;
            graphics::draw(ctx, &m, (na::Point2::new(-state.screen.x_offset, 0.0),))
        } else {
            let mut long_bt_builder = graphics::MeshBuilder::new();
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

                long_bt_builder.rectangle(graphics::DrawMode::fill(), [x, y, w, h].into(), color);
            }
            let m = long_bt_builder.build(ctx)?;
            graphics::draw(ctx, &m, (na::Point2::new(0.0, 0.0),))
        }
    }

    fn draw_ui(&mut self, ui: &Ui, actions: &mut ActionStack<Chart>) {}
}

impl CursorObject for LaserTool {
    fn mouse_down(
        &mut self,
        screen: ScreenState,
        tick: u32,
        tick_f: f64,
        lane: f32,
        chart: &Chart,
        actions: &mut ActionStack<Chart>,
        pos: na::Point2<f32>,
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
                            if na::distance(&control_point, &pos) < 5.0 {
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
        screen: ScreenState,
        _tick: u32,
        _tick_f: f64,
        _lane: f32,
        _chart: &Chart,
        actions: &mut ActionStack<Chart>,
        _pos: na::Point2<f32>,
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

    fn update(&mut self, tick: u32, tick_f: f64, lane: f32, pos: na::Point2<f32>) {
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
    fn draw(&self, state: &MainState, ctx: &mut Context) -> GameResult {
        if self.section.v.len() > 1 {
            //Draw laser mesh
            if let Some(color) = match self.mode {
                LaserEditMode::None => None,
                LaserEditMode::New => {
                    let b = 0.8;
                    if self.right {
                        [0.76 * b, 0.024 * b, 0.55 * b, 1.0].into()
                    } else {
                        [0.0, 0.45 * b, 0.565 * b, 1.0].into()
                    }
                }
                LaserEditMode::Edit(_) => Some([0.0, 0.76, 0.0, 1.0]),
            } {
                let mut mb = graphics::MeshBuilder::new();
                state.draw_laser_section(&self.section, &mut mb, color.into())?;
                graphics::set_blend_mode(ctx, graphics::BlendMode::Add)?;
                let m = mb.build(ctx)?;
                graphics::draw(ctx, &m, (na::Point2::new(-state.screen.x_offset, 0.0),))?;
            }

            //Draw curve control points
            if let LaserEditMode::Edit(edit_state) = self.mode {
                let mut mb = graphics::MeshBuilder::new();
                for (i, start_end) in self.section.v.windows(2).enumerate() {
                    let color = if edit_state.curving_index == Some(i) {
                        [0.0, 1.0, 0.0, 1.0]
                    } else {
                        [0.0, 0.0, 1.0, 1.0]
                    };

                    if let Some(pos) =
                        LaserTool::get_control_point_pos(state.screen, start_end, self.section.y)
                    {
                        mb.circle(graphics::DrawMode::fill(), pos, 5.0, 0.3, color.into());
                    }
                }
                let m = mb.build(ctx)?;
                graphics::set_blend_mode(ctx, graphics::BlendMode::Alpha)?;
                graphics::draw(ctx, &m, (na::Point2::new(0.0, 0.0),))?;
            }
        }
        Ok(())
    }
    fn draw_ui(&mut self, ui: &Ui, actions: &mut ActionStack<Chart>) {}
}

enum BpmToolStates {
    None,
    Add(u32),
    Edit(usize),
}

pub struct BpmTool {
    bpm: f64,
    state: BpmToolStates,
    cursor_tick: u32,
}

impl BpmTool {
    pub fn new() -> Self {
        BpmTool {
            bpm: 120.0,
            state: BpmToolStates::None,
            cursor_tick: 0,
        }
    }
}

impl CursorObject for BpmTool {
    fn mouse_down(
        &mut self,
        screen: ScreenState,
        tick: u32,
        tick_f: f64,
        lane: f32,
        chart: &Chart,
        actions: &mut ActionStack<Chart>,
        pos: na::Point2<f32>,
    ) {
        if let BpmToolStates::None = self.state {
            //check for bpm changes on selected tick
            for (i, change) in chart.beat.bpm.iter().enumerate() {
                if change.y == tick {
                    self.state = BpmToolStates::Edit(i);
                    self.bpm = change.v;
                    return;
                }
            }

            self.state = BpmToolStates::Add(tick);
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
        _pos: na::Point2<f32>,
    ) {
    }

    fn update(&mut self, tick: u32, _tick_f: f64, _lane: f32, _pos: na::Point2<f32>) {
        if let BpmToolStates::None = self.state {
            self.cursor_tick = tick;
        }
    }

    fn draw(&self, state: &MainState, ctx: &mut Context) -> GameResult {
        state.draw_cursor_line(ctx, self.cursor_tick, (0, 128, 255, 255))
    }

    fn draw_ui(&mut self, ui: &Ui, actions: &mut ActionStack<Chart>) {
        let complete_func: Option<Box<dyn Fn(&mut ActionStack<Chart>, f64) -> ()>> =
            match self.state {
                BpmToolStates::None => None,
                BpmToolStates::Add(tick) => {
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
                BpmToolStates::Edit(index) => {
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
            Window::new(im_str!("Change BPM"))
                .size([300.0, 600.0], imgui::Condition::FirstUseEver)
                .position([100.0, 100.0], imgui::Condition::FirstUseEver)
                .build(&ui, || {
                    InputFloat::new(ui, im_str!("BPM"), &mut bpm).build();
                    self.bpm = bpm as f64;
                    if Selectable::new(im_str!("Ok")).selected(false).build(ui) {
                        complete(actions, bpm as f64);
                        self.state = BpmToolStates::None;
                    }

                    if Selectable::new(im_str!("Cancel")).selected(false).build(ui) {
                        self.state = BpmToolStates::None;
                    }
                });
        }
    }
}

use crate::action_stack::{Action, ActionStack};
use crate::MainState;

use ggez::graphics;
use ggez::nalgebra as na;
use ggez::{Context, GameResult};
use kson::{Chart, GraphSectionPoint, Interval, LaserSection};

pub trait CursorObject {
    fn mouse_down(
        &mut self,
        tick: u32,
        tick_f: f64,
        lane: f32,
        chart: &Chart,
        actions: &mut ActionStack<Chart>,
    );
    fn mouse_up(
        &mut self,
        tick: u32,
        tick_f: f64,
        lane: f32,
        chart: &Chart,
        actions: &mut ActionStack<Chart>,
    );
    fn update(&mut self, tick: u32, tick_f: f64, lane: f32);
    fn draw(&self, state: &MainState, ctx: &mut Context) -> GameResult;
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

enum LaserEditMode {
    None,
    New,
    Edit(usize),
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
        tick: u32,
        tick_f: f64,
        lane: f32,
        chart: &Chart,
        actions: &mut ActionStack<Chart>,
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
        tick: u32,
        tick_f: f64,
        _lane: f32,
        chart: &Chart,
        actions: &mut ActionStack<Chart>,
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

    fn update(&mut self, tick: u32, tick_f: f64, lane: f32) {
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
            let (x, y) = state.tick_to_pos(self.interval.y);

            let x = if self.fx {
                x + self.lane as f32 * state.lane_width() * 2.0
                    + 2.0 * self.lane as f32
                    + state.lane_width()
                    + state.track_width / 2.0
            } else {
                x + self.lane as f32 * state.lane_width()
                    + 1.0 * self.lane as f32
                    + state.lane_width()
                    + state.track_width / 2.0
            };
            let y = y as f32;

            let w = if self.fx {
                state.track_width as f32 / 3.0 - 1.0
            } else {
                state.track_width as f32 / 6.0 - 2.0
            };
            let h = -2.0;

            let m = graphics::Mesh::new_rectangle(
                ctx,
                graphics::DrawMode::fill(),
                [x, y, w, h].into(),
                color,
            )?;
            graphics::draw(ctx, &m, (na::Point2::new(0.0, 0.0),))
        } else {
            let mut long_bt_builder = graphics::MeshBuilder::new();
            for (x, y, h, _) in state.interval_to_ranges(&self.interval) {
                let x = if self.fx {
                    x + self.lane as f32 * state.lane_width() * 2.0
                        + 2.0 * self.lane as f32
                        + state.lane_width()
                        + state.track_width / 2.0
                } else {
                    x + self.lane as f32 * state.lane_width()
                        + 1.0 * self.lane as f32
                        + state.lane_width()
                        + state.track_width / 2.0
                };

                let w = if self.fx {
                    state.track_width as f32 / 3.0 - 1.0
                } else {
                    state.track_width as f32 / 6.0 - 2.0
                };

                long_bt_builder.rectangle(graphics::DrawMode::fill(), [x, y, w, h].into(), color);
            }
            let m = long_bt_builder.build(ctx)?;
            graphics::draw(ctx, &m, (na::Point2::new(0.0, 0.0),))
        }
    }
}

impl CursorObject for LaserTool {
    fn mouse_down(
        &mut self,
        tick: u32,
        tick_f: f64,
        lane: f32,
        chart: &Chart,
        actions: &mut ActionStack<Chart>,
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
                    self.mode = LaserEditMode::Edit(section_index);
                } else {
                    self.section.y = tick;
                    self.section.v.push(GraphSectionPoint::new(0, v));
                    self.section.v.push(GraphSectionPoint::new(0, v));
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
                    .push(GraphSectionPoint::new(ry, LaserTool::lane_to_pos(lane)));
            }
            LaserEditMode::Edit(section_index) => {
                if self.hit_test(chart, tick) == Some(section_index) {
                    //do stuff
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
        _tick: u32,
        _tick_f: f64,
        _lane: f32,
        _chart: &Chart,
        _actions: &mut ActionStack<Chart>,
    ) {
    }
    fn update(&mut self, tick: u32, tick_f: f64, lane: f32) {
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
            LaserEditMode::Edit(_) => {}
        }
    }
    fn draw(&self, state: &MainState, ctx: &mut Context) -> GameResult {
        if self.section.v.len() > 1 {
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
                graphics::draw(ctx, &m, (na::Point2::new(0.0, 0.0),))?;
            }
        }
        Ok(())
    }
}

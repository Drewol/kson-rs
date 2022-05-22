use crate::tools::CursorObject;
use crate::Modifiers;
use crate::{
    action_stack::ActionStack,
    chart_editor::{MainState, ScreenState},
    utils::Overlaps,
};
use anyhow::Result;
use eframe::egui::{Painter, Pos2, Rgba, Stroke};
use eframe::epaint::Shape;
use kson::{Chart, GraphSectionPoint, LaserSection};

pub struct LaserTool {
    right: bool,
    section: LaserSection,
    mode: LaserEditMode,
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

    fn lane_to_pos(lane: f32, wide: u8) -> f64 {
        let resolution: f64 = 10.0 * wide as f64;
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

        chart.note.laser[side_index]
            .iter()
            .enumerate()
            .find(|(_, s)| s.contains(tick))
            .map(|(i, _)| i)
    }
}

impl CursorObject for LaserTool {
    fn drag_start(
        &mut self,
        screen: ScreenState,
        tick: u32,
        _tick_f: f64,
        lane: f32,
        chart: &Chart,
        actions: &mut ActionStack<Chart>,
        pos: Pos2,
        modifiers: &Modifiers,
    ) {
        let wide = modifiers.shift;
        let v = LaserTool::lane_to_pos(lane, if wide { 2 } else { 1 });
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
                    self.section.wide = if wide { 2 } else { 1 };
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
                    let v = std::rc::Rc::new(v); //Can't capture by clone so use RC
                    let i = if self.right { 1 } else { 0 };
                    let new_action = actions.new_action();
                    new_action.description =
                        format!("Add {} Laser", if self.right { "Right" } else { "Left" });
                    new_action.action = Box::new(move |edit_chart| {
                        edit_chart.note.laser[i].push(v.as_ref().clone());
                        edit_chart.note.laser[i].sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
                        Ok(())
                    });

                    return;
                }

                self.section.v.push(LaserTool::gsp(
                    ry,
                    LaserTool::lane_to_pos(lane, self.section.wide),
                ));
            }
            LaserEditMode::Edit(edit_state) => {
                if self.hit_test(chart, tick) == Some(edit_state.section_index) {
                    for (i, points) in self.section.v.windows(2).enumerate() {
                        if let Some(control_point) = screen.get_control_point_pos_section(
                            points,
                            self.section.y,
                            (0.0, 1.0),
                            Some((0.5 / 6.0, 5.5 / 6.0)),
                        ) {
                            if control_point.distance(pos) < 5.0 {
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
    fn drag_end(
        &mut self,
        _screen: ScreenState,
        _tick: u32,
        _tick_f: f64,
        _lane: f32,
        _chart: &Chart,
        actions: &mut ActionStack<Chart>,
        _pos: Pos2,
    ) {
        if let LaserEditMode::Edit(edit_state) = self.mode {
            if let Some(curving_index) = edit_state.curving_index {
                let right = self.right;
                let laser_text = if right { "Right" } else { "Left" };
                let section_index = edit_state.section_index;
                let laser_i = if right { 1 } else { 0 };
                let updated_point = self.section.v[curving_index];

                let new_action = actions.new_action();
                new_action.description = format!("Adjust {} Laser Curve", laser_text);
                new_action.action = Box::new(move |c| {
                    c.note.laser[laser_i][section_index].v[curving_index] = updated_point;
                    Ok(())
                });
            }
            self.mode = LaserEditMode::Edit(LaserEditState {
                section_index: edit_state.section_index,
                curving_index: None,
            })
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
        if let Some(index) = self.hit_test(chart, tick) {
            let laser_i = if self.right { 1 } else { 0 };
            let new_action = actions.new_action();
            new_action.description =
                format!("Remove {} laser", if self.right { "right" } else { "left" });
            new_action.action = Box::new(move |chart: &mut Chart| {
                chart.note.laser[laser_i].remove(index);
                Ok(())
            });
        }
    }

    fn update(&mut self, tick: u32, tick_f: f64, lane: f32, _pos: Pos2, _chart: &Chart) {
        match self.mode {
            LaserEditMode::New => {
                let ry = self.calc_ry(tick);
                let v = LaserTool::lane_to_pos(lane, self.section.wide);
                let second_last: Option<GraphSectionPoint> = self.get_second_to_last().copied();
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
                    let in_value = lane as f64 / 5.0 - 0.5 / 6.0;
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
                LaserEditMode::Edit(_) => Some(Rgba::from_rgba_premultiplied(0.0, 0.76, 0.0, 0.25)),
            } {
                let mut mb = Vec::new();
                state
                    .screen
                    .draw_laser_section(&self.section, &mut mb, color.into(), false)?;
                painter.extend(mb.into_iter().map(Shape::mesh).collect());
            }

            //Draw curve control points
            if let LaserEditMode::Edit(edit_state) = self.mode {
                for (i, start_end) in self.section.v.windows(2).enumerate() {
                    let color = if edit_state.curving_index == Some(i) {
                        Rgba::from_rgba_premultiplied(0.0, 1.0, 0.0, 1.0)
                    } else {
                        Rgba::from_rgba_premultiplied(0.0, 0.0, 1.0, 1.0)
                    };

                    if let Some(pos) = state.screen.get_control_point_pos_section(
                        start_end,
                        self.section.y,
                        (0.0, 1.0),
                        Some((0.5 / 6.0, 5.5 / 6.0)),
                    ) {
                        painter.circle(pos, 5.0, color, Stroke::none());
                    }
                }
            }
        }
        Ok(())
    }
}

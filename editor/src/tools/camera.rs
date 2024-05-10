use eframe::{
    egui::{vec2, Color32, ComboBox, Pos2, Slider, Stroke},
    epaint::Rgba,
};

use crate::i18n;
use glam::vec3;
use kson::{Chart, Graph, GraphPoint, GraphSectionPoint};
use std::{default::Default, f32::EPSILON, fmt::Display, ops::Sub};

use crate::camera_widget::CameraView;
use crate::chart_camera::ChartCamera;

use super::CursorObject;

#[derive(Debug, PartialEq, Clone, Copy)]
enum CameraPaths {
    Zoom,
    RotationX,
}

impl Default for CameraPaths {
    fn default() -> Self {
        Self::Zoom
    }
}

impl Display for CameraPaths {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            CameraPaths::Zoom => formatter.write_str(&i18n::fl!("radius")),
            CameraPaths::RotationX => formatter.write_str(&i18n::fl!("angle")),
        }
    }
}

#[derive(Debug, Default)]
pub struct CameraTool {
    radius: f32,
    angle: f32,
    angle_dirty: bool,
    radius_dirty: bool,
    display_line: CameraPaths,
    curving_index: Option<(usize, f64, f64)>,
}

impl CameraTool {
    fn current_graph<'a>(&mut self, chart: &'a kson::Chart) -> &'a Vec<kson::GraphPoint> {
        match self.display_line {
            CameraPaths::Zoom => &chart.camera.cam.body.zoom,
            CameraPaths::RotationX => &chart.camera.cam.body.rotation_x,
        }
    }
}

impl CursorObject for CameraTool {
    fn update(&mut self, _tick: u32, tick_f: f64, lane: f32, _pos: Pos2, chart: &Chart) {
        if let Some((c_idx, _, _)) = self.curving_index {
            let transform_value = |v: f64| (v + 3.0) / 6.0;

            if let Some(section) = self.current_graph(chart).windows(2).nth(c_idx) {
                let a = tick_f - section[0].y as f64;
                let a = a / (section[1].y - section[0].y) as f64;

                //TODO: map b value to match mouse position better
                let point = &section[0];
                let end_point = &section[1];
                let start_value = transform_value(point.vf.unwrap_or(point.v));
                let in_value = lane as f64 / 6.0;
                let value = (in_value - start_value) / (transform_value(end_point.v) - start_value);

                self.curving_index = Some((c_idx, a.max(0.0).min(1.0), value.max(0.0).min(1.0)));
            }
        }
    }

    fn draw(
        &self,
        state: &crate::chart_editor::MainState,
        painter: &eframe::egui::Painter,
    ) -> anyhow::Result<()> {
        let (graph, stroke) = match self.display_line {
            CameraPaths::Zoom => (
                &state.chart.camera.cam.body.zoom,
                Stroke::new(1.0, Rgba::from_rgb(1.0, 1.0, 0.0)),
            ),
            CameraPaths::RotationX => (
                &state.chart.camera.cam.body.rotation_x,
                Stroke::new(1.0, Rgba::from_rgb(0.0, 1.0, 1.0)),
            ),
        };

        state.draw_graph(graph, painter, (-3.0, 3.0), stroke);

        for (i, start_end) in graph.windows(2).enumerate() {
            let (color, points) = if matches!(self.curving_index, Some((ci, _, _)) if ci == i) {
                let new_start = if let Some((_, a, b)) = self.curving_index {
                    GraphPoint {
                        y: start_end[0].y,
                        v: start_end[0].v,
                        vf: start_end[0].vf,
                        a: Some(a),
                        b: Some(b),
                    }
                } else {
                    start_end[0]
                };

                (
                    Rgba::from_rgba_premultiplied(0.0, 1.0, 0.0, 1.0),
                    [new_start, start_end[1]],
                )
            } else {
                (
                    Rgba::from_rgba_premultiplied(0.0, 0.0, 1.0, 1.0),
                    [start_end[0], start_end[1]],
                )
            };

            if let Some(pos) = state
                .screen
                .get_control_point_pos(&points, (-3.0, 3.0), None)
            {
                painter.circle(pos, 5.0, color, Stroke::NONE);
            }
        }

        if let Some((c_idx, a, b)) = self.curving_index {
            if let Some(points) = graph.windows(2).nth(c_idx) {
                state.draw_graph_segmented(
                    &points
                        .iter()
                        .map(|p| GraphSectionPoint {
                            ry: p.y,
                            v: p.v,
                            vf: p.vf,
                            a: Some(a),
                            b: Some(b),
                        })
                        .collect::<Vec<_>>(),
                    painter,
                    (-3.0, 3.0),
                    Stroke {
                        width: 1.0,
                        color: Color32::GREEN,
                    },
                );
            }
        }

        Ok(())
    }

    fn drag_start(
        &mut self,
        screen: crate::chart_editor::ScreenState,
        _tick: u32,
        _tick_f: f64,
        _lane: f32,
        chart: &kson::Chart,
        _actions: &mut crate::action_stack::ActionStack<kson::Chart>,
        pos: Pos2,
        _modifiers: &crate::Modifiers,
    ) {
        let graph = self.current_graph(chart);

        for (i, points) in graph.windows(2).enumerate() {
            if let Some(control_point) = screen.get_control_point_pos(points, (-3.0, 3.0), None) {
                if control_point.distance(pos) < 5.0 {
                    self.curving_index =
                        Some((i, points[0].a.unwrap_or(0.5), points[0].b.unwrap_or(0.5)));
                }
            }
        }
    }

    fn drag_end(
        &mut self,
        _screen: crate::chart_editor::ScreenState,
        _tick: u32,
        _tick_f: f64,
        _lane: f32,
        _chart: &kson::Chart,
        actions: &mut crate::action_stack::ActionStack<kson::Chart>,
        _pos: Pos2,
    ) {
        if let Some((ci, a, b)) = self.curving_index {
            let new_action = actions.new_action();
            let active_line = self.display_line;
            new_action.action = Box::new(move |chart| {
                let graph = match active_line {
                    CameraPaths::Zoom => &mut chart.camera.cam.body.zoom,
                    CameraPaths::RotationX => &mut chart.camera.cam.body.rotation_x,
                };

                if let Some(point) = graph.get_mut(ci) {
                    point.a = Some(a);
                    point.b = Some(b);
                }

                Ok(())
            });

            new_action.description = i18n::fl!(
                "edit_curve_for_camera",
                graph = match self.display_line {
                    CameraPaths::Zoom => i18n::fl!("radius"),
                    CameraPaths::RotationX => i18n::fl!("angle"),
                }
            )
        }

        self.curving_index = None
    }

    fn draw_ui(&mut self, state: &mut crate::chart_editor::MainState, ctx: &eframe::egui::Context) {
        //Draw winodw, with a viewport that uses the ChartCamera to project a track in using current camera parameters.
        let cursor_tick = state.get_current_cursor_tick() as f64;

        let old_rad = if self.radius_dirty {
            self.radius
        } else {
            state.chart.camera.cam.body.zoom.value_at(cursor_tick) as f32
        };

        let old_angle = if self.angle_dirty {
            self.angle
        } else {
            state.chart.camera.cam.body.rotation_x.value_at(cursor_tick) as f32
        };

        self.angle = old_angle;
        self.radius = old_rad;

        let camera = ChartCamera {
            center: vec3(0.0, 0.0, 0.0),
            angle: -45.0 - 14.0 * self.angle,
            fov: 70.0,
            radius: (-self.radius + 3.1) / 2.0,
            tilt: 0.0,
            track_length: 16.0,
        };

        eframe::egui::Window::new(i18n::fl!("camera"))
            .title_bar(true)
            .open(&mut true)
            .resizable(true)
            .show(ctx, |ui| {
                let mut camera_view = CameraView::new(vec2(300.0, 200.0), camera);
                camera_view.add_track(&state.laser_colors);
                camera_view.add_chart_objects(
                    &state.chart,
                    cursor_tick as f32,
                    &state.laser_colors,
                );
                camera_view.add_track_overlay();
                ui.add(camera_view);
                ui.add(Slider::new(&mut self.radius, -3.0..=3.0).text(i18n::fl!("radius")));
                ui.add(Slider::new(&mut self.angle, -3.0..=3.0).text(i18n::fl!("angle")));

                if old_angle.sub(self.angle).abs() > EPSILON {
                    self.angle_dirty = true;
                }

                if old_rad.sub(self.radius).abs() > EPSILON {
                    self.radius_dirty = true;
                }

                ComboBox::from_label(i18n::fl!("display_line"))
                    .selected_text(self.display_line.to_string())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.display_line,
                            CameraPaths::Zoom,
                            CameraPaths::Zoom.to_string(),
                        );
                        ui.selectable_value(
                            &mut self.display_line,
                            CameraPaths::RotationX,
                            CameraPaths::RotationX.to_string(),
                        );
                    });

                if ui.button(i18n::fl!("add_control_point")).clicked() {
                    let new_action = state.actions.new_action();
                    new_action.description = i18n::fl!("added_camera_control_point").to_string();
                    let Self {
                        angle,
                        radius,
                        radius_dirty,
                        angle_dirty,
                        display_line: _,
                        curving_index: _,
                    } = *self;
                    let y = state.cursor_line;
                    new_action.action = Box::new(move |c| {
                        if angle_dirty {
                            c.camera.cam.body.rotation_x.push(kson::GraphPoint {
                                y,
                                v: angle as f64,
                                vf: None,
                                a: Some(0.5),
                                b: Some(0.5),
                            })
                        }
                        if radius_dirty {
                            c.camera.cam.body.zoom.push(kson::GraphPoint {
                                y,
                                v: radius as f64,
                                vf: None,
                                a: Some(0.5),
                                b: Some(0.5),
                            });
                        }

                        //TODO: just insert sorted instead
                        c.camera.cam.body.zoom.sort_by_key(|p| p.y);
                        c.camera.cam.body.rotation_x.sort_by_key(|p| p.y);
                        Ok(())
                    });

                    self.radius_dirty = false;
                    self.angle_dirty = false;
                }
            });
    }
}

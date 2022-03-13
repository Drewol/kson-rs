use std::{f32::EPSILON, ops::Sub};

use eframe::egui::{pos2, vec2, Color32, Sense, Slider, Stroke, Vec2, Widget};
use glam::{vec3, vec4, Mat4};
use kson::Graph;
use nalgebra::ComplexField;

use crate::chart_camera::ChartCamera;

use super::CursorObject;

#[derive(Debug, Default)]
pub struct CameraTool {
    radius: f32,
    angle: f32,
    angle_dirty: bool,
    radius_dirty: bool,
}

struct CamreaView {
    desired_size: Vec2,
    camera: ChartCamera,
}

impl Widget for CamreaView {
    fn ui(self, ui: &mut eframe::egui::Ui) -> eframe::egui::Response {
        let (response, painter) = ui.allocate_painter(self.desired_size, Sense::click());
        let view_rect = response.rect;
        let size = view_rect.size();
        let (projection, camera_transform) = self.camera.matrix(size);
        painter.rect(
            ui.max_rect(),
            0.0,
            Color32::from_rgb(0, 0, 0),
            Stroke::none(),
        );

        let points = [
            vec3(-0.5, 0.0, 0.0),
            vec3(-0.5, 0.0, 10.0),
            vec3(0.5, 0.0, 10.0),
            vec3(0.5, 0.0, 0.0),
            vec3(-0.5, 0.0, 0.0),
        ]
        .map(|p| Mat4::from_rotation_y(90_f32.to_radians()).transform_point3(p))
        .map(|p| projection.project_point3(camera_transform.transform_point3(p)))
        .map(|p| p * vec3(1.0, -1.0, 1.0))
        .map(|p| p + vec3(1.0, 1.0, 1.0))
        .map(|p| p / vec3(2.0, 2.0, 2.0))
        .map(|p| p);

        for segment in points.windows(2) {
            let pos = [
                pos2(segment[0].x * size.x, segment[0].y * size.y) + view_rect.left_top().to_vec2(),
                pos2(segment[1].x * size.x, segment[1].y * size.y) + view_rect.left_top().to_vec2(),
            ];
            painter.line_segment(pos, Stroke::new(2.0, Color32::from_rgb(255, 0, 0)))
        }

        response
    }
}

impl CursorObject for CameraTool {
    fn update(&mut self, _tick: u32, _tick_f: f64, _lane: f32, _pos: nalgebra::Point2<f32>) {}

    fn draw(
        &self,
        _state: &crate::chart_editor::MainState,
        _painter: &eframe::egui::Painter,
    ) -> anyhow::Result<()> {
        //TODO: Visualize camera values on track
        Ok(())
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
            radius: 3.1 - self.radius,
            tilt: 0.0,
            track_length: 16.0,
        };

        eframe::egui::Window::new("Camera")
            .title_bar(true)
            .open(&mut true)
            .resizable(true)
            .show(ctx, |ui| {
                ui.add(CamreaView {
                    camera,
                    desired_size: vec2(300.0, 200.0),
                });

                ui.add(Slider::new(&mut self.radius, -3.0..=3.0).text("Radius"));
                ui.add(Slider::new(&mut self.angle, -3.0..=3.0).text("Angle"));

                if old_angle.sub(self.angle).abs() > EPSILON {
                    self.angle_dirty = true;
                }

                if old_rad.sub(self.radius).abs() > EPSILON {
                    self.radius_dirty = true;
                }

                if ui.button("Add Control Point").clicked() {
                    let new_action = state.actions.new_action();
                    new_action.description = "Added camera control point".to_string();
                    let Self {
                        angle,
                        radius,
                        radius_dirty,
                        angle_dirty,
                    } = *self;
                    let y = state.cursor_line;
                    new_action.action = Box::new(move |c| {
                        if angle_dirty {
                            c.camera.cam.body.rotation_x.push(kson::GraphPoint {
                                y,
                                v: angle as f64,
                                vf: None,
                                a: None,
                                b: None,
                            })
                        }
                        if radius_dirty {
                            c.camera.cam.body.zoom.push(kson::GraphPoint {
                                y,
                                v: radius as f64,
                                vf: None,
                                a: None,
                                b: None,
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

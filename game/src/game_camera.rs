use three_d::Camera;
use three_d::{Vec2, Vec3};
use three_d_asset::{Deg, InnerSpace, Rad, Viewport};

use crate::game::ChartView;

#[derive(Debug, Clone)]
pub struct ChartCamera {
    pub fov: f32,
    pub radius: f32,
    pub angle: f32,
    pub center: Vec3,
    pub track_length: f32,
    pub tilt: f32,
    pub view_size: Vec2,
    pub shakes: Vec<CameraShake>,
}

#[derive(Debug, Clone, Copy)]
pub struct CameraShake {
    amplitude: f32,
    direction: f32,
    frequency: f32,
    timer: f32,
    duration: f32,
}

impl CameraShake {
    pub fn new(amplitude: f32, direction: f32, frequency: f32, duration: f32) -> Self {
        Self {
            amplitude,
            direction,
            frequency,
            timer: duration,
            duration,
        }
    }

    pub fn get_shake(&self) -> f32 {
        (self.timer * self.frequency).sin()
            * self.direction
            * self.amplitude
            * (self.timer / self.duration).powf(2.0)
    }

    pub fn tick(&mut self, dt: f32) {
        self.timer -= dt;
        self.timer = self.timer.max(0.0);
    }

    pub fn completed(&self) -> bool {
        self.timer == 0.0
    }
}

impl ChartCamera {
    pub fn egui_widget(&mut self, ui: &mut egui::Ui) -> egui::Response {
        egui::Grid::new("camera_widget")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("FOV");
                ui.add(egui::Slider::new(&mut self.fov, 0.0..=180.0));
                ui.end_row();

                ui.label("Angle");
                ui.add(egui::Slider::new(&mut self.angle, -180.0..=180.0));
                ui.end_row();

                ui.label("Tilt");
                ui.add(egui::Slider::new(&mut self.tilt, -180.0..=180.0));
                ui.end_row();

                ui.label("Radius");
                ui.add(egui::Slider::new(&mut self.radius, 0.0..=10.0));
                ui.end_row();
            })
            .response
    }
}

impl From<&ChartCamera> for Camera {
    fn from(val: &ChartCamera) -> Self {
        let angle = val.angle.to_radians();
        let base_angle = val.fov.to_radians() / 2.2;

        let track_end: Vec3 = ChartView::TRACK_DIRECTION * val.track_length;
        let final_camera_pos: Vec3 = -ChartView::UP * val.radius * angle.to_radians().cos()
            + ChartView::TRACK_DIRECTION * val.radius * angle.to_radians().sin();

        let position = ChartView::TRACK_DIRECTION * val.radius;

        let end_dist = (track_end - final_camera_pos).magnitude();
        let begin_dist = (-track_end - final_camera_pos).magnitude();

        let z_far = end_dist.max(begin_dist);

        let target: Vec3 = val.center - ChartView::TRACK_DIRECTION;

        let mut cam = Camera::new_perspective(
            Viewport::new_at_origo(val.view_size.x as u32, val.view_size.y as u32),
            position,
            target,
            ChartView::UP,
            Deg(val.fov),
            ChartView::Z_NEAR,
            z_far,
        );

        cam.pitch(Rad(base_angle));
        cam.rotate_around_with_fixed_up(&val.center, 0.0, angle);
        cam.roll(Deg(val.tilt));
        cam.yaw(Rad(val.shakes.iter().map(|x| x.get_shake()).sum()));

        cam
    }
}

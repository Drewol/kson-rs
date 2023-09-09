use eframe::egui::Vec2;
use glam::{Mat4, Vec3};

#[derive(Debug, Clone, Copy)]
pub struct ChartCamera {
    pub fov: f32,
    pub radius: f32,
    pub angle: f32,
    pub center: Vec3,
    pub track_length: f32,
    pub tilt: f32,
}

impl ChartCamera {
    const UP: Vec3 = Vec3::Y;
    const TRACK_DIRECTION: Vec3 = Vec3::X;
    const Z_NEAR: f32 = 0.01;
}

impl ChartCamera {
    pub fn matrix(&self, view_size: Vec2) -> Mat4 {
        let camera_rotation_axis: Vec3 = Vec3::ONE - Self::TRACK_DIRECTION - Self::UP;
        let aspect = view_size.x / view_size.y;
        let angle = self.angle.to_radians();
        let base_angle = self.fov.to_radians() / 2.2;

        let track_end: Vec3 = Self::TRACK_DIRECTION * self.track_length;
        let final_camera_angle = -angle - (self.fov.to_radians() / 2.2);
        let final_camera_pos: Vec3 = -Self::UP * self.radius * angle.sin()
            + Self::TRACK_DIRECTION * self.radius * angle.cos();
        let camera_unit: Vec3 =
            -Self::TRACK_DIRECTION * final_camera_angle.cos() - Self::UP * final_camera_angle.sin();

        let neg_angle = -angle - base_angle;
        let tilt_unit: Vec3 = Self::TRACK_DIRECTION * neg_angle.cos() + Self::UP * neg_angle.sin();

        let tilt = Mat4::from_translation(self.center)
            * Mat4::from_axis_angle(tilt_unit, self.tilt.to_radians())
            * Mat4::from_translation(-self.center);

        let position = Mat4::from_translation(Self::TRACK_DIRECTION * self.radius);

        let end_dist = -camera_unit.dot(track_end + final_camera_pos);
        let begin_dist = -camera_unit.dot(-track_end + final_camera_pos);

        let z_far = end_dist.max(begin_dist);

        let rotation = Mat4::from_axis_angle(camera_rotation_axis, -angle);
        let base_rotation =
            Mat4::from_axis_angle(camera_rotation_axis, -self.fov.to_radians() / 2.2);

        let target: Vec3 = self.center + Self::TRACK_DIRECTION;
        Mat4::perspective_rh_gl(self.fov.to_radians(), aspect, Self::Z_NEAR, z_far)
            * (Mat4::look_at_rh(self.center, target, Self::UP) * tilt * (base_rotation * position))
            * (rotation)
    }
}

#[allow(unused)]
fn create_perspective(field_of_view: f32, aspect_ratio: f32, z_near: f32, z_far: f32) -> Mat4 {
    let mut result = [0_f32; 16];

    let height = z_near * f32::tan((field_of_view.to_radians()) * 0.5);
    let width = height * aspect_ratio;

    let f1 = z_near * 2.0;
    let f2 = width * 2.0;
    let f3 = height * 2.0;
    let f4 = z_far - z_near;
    result[0] = f1 / f2;
    result[5] = f1 / f3;
    result[10] = (-z_far - z_near) / f4;
    result[11] = -1.0;
    result[14] = (-f1 * z_far) / f4;
    result[15] = 0.0;

    Mat4::from_cols_array(&result)
}

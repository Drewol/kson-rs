use three_d_asset::{vec2, Camera, Deg, InnerSpace, Matrix4, Rad, Transform, Vec2, Vec3, Viewport};

use super::chart_view;
use chart_view::ChartView;

#[derive(Debug, Clone)]
pub struct ChartCamera {
    pub kson_radius: f32,
    pub kson_angle: f32,
    pub tilt: f32,
    pub view_size: Vec2,
    pub shakes: Vec<CameraShake>,
    pub spins: Vec<CameraSpin>,
    pub portrait: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum CameraSpin {
    Half(kson::camera::CamPatternInvokeSpin),
    Full(kson::camera::CamPatternInvokeSpin),
    Swing(kson::camera::CamPatternInvokeSwing),
}

impl CameraSpin {
    pub fn active_at(&self, tick: u32) -> bool {
        match self {
            CameraSpin::Half(s) => s.0 <= tick && s.0 + s.2 >= tick,
            CameraSpin::Full(s) => s.0 <= tick && s.0 + s.2 >= tick,
            CameraSpin::Swing(s) => s.0 <= tick && s.0 + s.2 >= tick,
        }
    }

    pub fn roll_at(self, tick: f32) -> f32 {
        match self {
            CameraSpin::Half(_) => 0.0,
            CameraSpin::Full(kson::camera::CamPatternInvokeSpin(y, dir, len)) => {
                //Reference https://github.com/kshootmania/ksm-v2/blob/master/kshootmania/src/music_game/camera/cam_pattern/cam_pattern_spin.cpp#L52
                let rate = (tick - y as f32) / len as f32;
                if !(0.0..=1.0).contains(&rate) {
                    return 0.0;
                }

                let abs_degrees = if rate < 360.0 / 675.0 {
                    (rate / (360.0 / 675.0) * 0.75).sin() / (0.75f32).sin() * 360.0
                } else if rate < 440.0 / 675.0 {
                    (((rate * 675.0 - 360.0) * 9.0 / 8.0).to_radians()).sin() * 30.0
                } else {
                    (1.0 - ((1.0 - rate) * 90.0 / 235.0 * 675.0)
                        .to_radians()
                        .cos()
                        .powf(2.0))
                        * 30.0
                };

                let dir = -(dir as f32).signum();
                abs_degrees * dir
            }
            CameraSpin::Swing(_) => 0.0,
        }
    }
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

// Statics for easier testing of values during dev
#[cfg(feature = "camera_test")]
mod camera_consts {
    pub static mut FOV_LANDSCAPE: f32 = 40.0;
    pub static mut FOV_PORTRAIT: f32 = 70.0;
    pub static mut ANGLE_LANDSCAPE: f32 = -59.0;
    pub static mut ANGLE_PORTRAIT: f32 = -59.0;
    pub static mut RADIUS_LANDSCAPE: f32 = 2.0;
    pub static mut RADIUS_PORTRAIT: f32 = 1.7;
}

#[cfg(not(feature = "camera_test"))]
mod camera_consts {
    pub const FOV_LANDSCAPE: f32 = 40.0;
    pub const FOV_PORTRAIT: f32 = 70.0;
    pub const ANGLE_LANDSCAPE: f32 = -59.0;
    pub const ANGLE_PORTRAIT: f32 = -59.0;
    pub const RADIUS_LANDSCAPE: f32 = 2.0;
    pub const RADIUS_PORTRAIT: f32 = 1.7;
}

use camera_consts::*;

impl Default for ChartCamera {
    fn default() -> Self {
        Self::new()
    }
}

impl ChartCamera {
    pub fn new() -> Self {
        ChartCamera {
            kson_angle: 0.0,
            kson_radius: 0.0,
            tilt: 0.0,
            view_size: vec2(2.0, 1.0),
            shakes: vec![],
            spins: vec![],
            portrait: false,
        }
    }

    fn base_values(&self) -> (f32, f32, f32) {
        #[cfg(feature = "camera_test")]
        unsafe {
            if self.portrait {
                (FOV_PORTRAIT, ANGLE_PORTRAIT, RADIUS_PORTRAIT)
            } else {
                (FOV_LANDSCAPE, ANGLE_LANDSCAPE, RADIUS_LANDSCAPE)
            }
        }

        #[cfg(not(feature = "camera_test"))]
        {
            if self.portrait {
                (FOV_PORTRAIT, ANGLE_PORTRAIT, RADIUS_PORTRAIT)
            } else {
                (FOV_LANDSCAPE, ANGLE_LANDSCAPE, RADIUS_LANDSCAPE)
            }
        }
    }

    pub fn update(&mut self, view_size: glam::Vec2) {
        self.portrait = (view_size.x / view_size.y) < 1.0;

        self.view_size = vec2(view_size.x, view_size.y);
    }

    pub fn check_spins(&mut self, tick: u32) {
        self.spins.retain(|x| x.active_at(tick))
    }

    pub fn egui_widget(&mut self, ui: &mut egui::Ui) -> egui::Response {
        egui::Grid::new("camera_widget")
            .num_columns(2)
            .show(ui, |ui| {
                #[cfg(feature = "camera_test")]
                {
                    let (fov, angle, radius) = unsafe {
                        if self.portrait {
                            (&mut FOV_PORTRAIT, &mut ANGLE_PORTRAIT, &mut RADIUS_PORTRAIT)
                        } else {
                            (
                                &mut FOV_LANDSCAPE,
                                &mut ANGLE_LANDSCAPE,
                                &mut RADIUS_LANDSCAPE,
                            )
                        }
                    };

                    ui.label("FOV");
                    ui.add(egui::Slider::new(fov, 0.1..=179.0));
                    ui.end_row();

                    ui.label("Angle");
                    ui.add(egui::Slider::new(angle, -360.0..=360.0));
                    ui.end_row();

                    ui.label("Radius");
                    ui.add(egui::Slider::new(radius, 0.0..=10.0));
                    ui.end_row();
                }

                ui.label("Tilt");
                ui.add(egui::Slider::new(&mut self.tilt, -360.0..=360.0));
                ui.end_row();
            })
            .response
    }
}

const KSON_ANGLE_FACTOR: f32 = 360.0 / 2400.0;

impl From<&ChartCamera> for Camera {
    fn from(val: &ChartCamera) -> Self {
        let (fov, angle, radius) = val.base_values();

        let radius = radius * f32::powf(3.0, val.kson_radius / -300.0);
        let angle = angle + (val.kson_angle * KSON_ANGLE_FACTOR);

        let angle_rad = (angle).to_radians();
        let fov_rad = fov.to_radians();

        //Wrong, idk why, doesn't really matter once correct values are found though
        let base_angle_rad =
            { (fov_rad) / (2.0) - (if val.portrait { 0.27 } else { 0.05 } * fov_rad) };

        let track_end: glam::Vec3 = ChartView::TRACK_DIRECTION * ChartView::TRACK_LENGTH;
        let final_camera_pos: glam::Vec3 = -ChartView::UP * radius * angle_rad.cos()
            + ChartView::TRACK_DIRECTION * radius * angle_rad.sin();

        // let up = (ChartView::UP * radius * (angle_rad).sin()
        //     + ChartView::TRACK_DIRECTION * radius * (angle_rad).cos());

        let up = if final_camera_pos.y >= 0.0 {
            ChartView::UP
        } else {
            -ChartView::UP
        };

        let target = final_camera_pos
            + (ChartView::UP * (angle_rad - base_angle_rad).cos()
                - ChartView::TRACK_DIRECTION * (angle_rad - base_angle_rad).sin());

        let end_dist = (track_end - final_camera_pos).length();
        let begin_dist = (-track_end - final_camera_pos).length();
        let z_far = end_dist.max(begin_dist);

        let roll =
            Matrix4::from_axis_angle(ChartView::TRACK_DIRECTION.to_array().into(), Deg(-val.tilt));
        let target = roll.transform_vector((target - final_camera_pos).to_array().into())
            + Vec3::from(final_camera_pos.to_array());
        let up = roll.transform_vector(up.to_array().into());
        // let final_camera_pos = roll.transform_vector(final_camera_pos);

        let mut cam = Camera::new_perspective(
            Viewport::new_at_origo(val.view_size.x as u32, val.view_size.y as u32),
            final_camera_pos.to_array().into(),
            target,
            up.normalize(),
            Rad(fov_rad),
            ChartView::Z_NEAR,
            z_far,
        );
        // cam.roll(Deg(val.tilt)); //TODO: Need to roll the position and stuff now
        cam.yaw(Rad(val.shakes.iter().map(|x| x.get_shake()).sum()));

        cam
    }
}

pub trait WgpuCameraWrapper {
    fn wgpu_view(&self) -> [f32; 16];
    fn wgpu_proj(&self) -> [f32; 16];
}

impl WgpuCameraWrapper for Camera {
    fn wgpu_view(&self) -> [f32; 16] {
        let view = self.view();
        glam::Mat4::from_cols_array(&[
            view.x.x, view.x.y, view.x.z, view.x.w, view.y.x, view.y.y, view.y.z, view.y.w,
            view.z.x, view.z.y, view.z.z, view.z.w, view.w.x, view.w.y, view.w.z, view.w.w,
        ])
        .to_cols_array()
    }

    fn wgpu_proj(&self) -> [f32; 16] {
        let proj = self.projection();
        (glam::Mat4::from_cols_array(&[
            proj.x.x, proj.x.y, proj.x.z, proj.x.w, proj.y.x, proj.y.y, proj.y.z, proj.y.w,
            proj.z.x, proj.z.y, proj.z.z, proj.z.w, proj.w.x, proj.w.y, proj.w.z, proj.w.w,
        ]))
        .to_cols_array()
    }
}

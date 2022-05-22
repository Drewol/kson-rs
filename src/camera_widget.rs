use bytemuck::offset_of;
use eframe::{
    egui::{Sense, Widget},
    epaint::{color::Hsva, Color32, PaintCallback, Stroke, Vertex},
    glow::{HasContext, NativeBuffer},
};
use egui_glow::check_for_gl_error;
use emath::{pos2, vec2, Rect, Vec2};
use kson::Chart;
use once_cell::sync::OnceCell;

use crate::{assets, chart_camera::ChartCamera};

pub enum Material {
    Track,
    ChipBT,
    ChipFX,
    LongBT,
    LongFX,
    Laser(u8),
}

pub struct Mesh {
    mesh: eframe::epaint::Mesh,
    material: Material,
}

pub struct CameraView {
    desired_size: Vec2,
    camera: ChartCamera,
    meshes: Vec<Mesh>,
}

impl CameraView {
    const TRACK_LENGH: f32 = 16.0;
    const TRACK_WIDTH: f32 = 1.0;

    pub fn new(desired_size: Vec2, camera: ChartCamera) -> Self {
        Self {
            desired_size,
            camera,
            meshes: Default::default(),
        }
    }

    pub fn add_track(&mut self) {
        let left = -(Self::TRACK_WIDTH / 2.0);
        let right = Self::TRACK_WIDTH / 2.0;

        let mut track_mesh = eframe::epaint::Mesh::with_texture(Default::default());

        track_mesh.add_rect_with_uv(
            Rect {
                min: pos2(left, 0.0),
                max: pos2(right, Self::TRACK_LENGH),
            },
            Rect {
                min: pos2(0.0, 0.5),
                max: pos2(1.0, 0.5),
            },
            Color32::from_gray(255),
        );

        self.meshes.push(Mesh {
            mesh: track_mesh,
            material: Material::Track,
        });
    }
    pub fn add_mesh(&mut self, mesh: Mesh) {
        self.meshes.push(mesh)
    }

    pub fn add_chart_objects(&mut self, chart: &Chart, tick: u32) {}
}

impl Widget for CameraView {
    fn ui(self, ui: &mut eframe::egui::Ui) -> eframe::egui::Response {
        let width = ui.available_size_before_wrap().x.max(self.desired_size.x);
        let height = width / (16.0 / 9.0); //16:9 aspect ratio, potentially allow toggle to 9:16
        ui.ctx().request_repaint();
        let time = ui.ctx().input().time;
        let (response, painter) = ui.allocate_painter(vec2(width, height), Sense::click());
        let view_rect = response.rect;
        let size = view_rect.size();
        let projection = self.camera.matrix(size);
        painter.rect(
            ui.max_rect(),
            0.0,
            Color32::from_rgb(0, 0, 0),
            Stroke::none(),
        );

        for mesh in self.meshes {
            let proj = projection.to_cols_array();
            let callback = PaintCallback {
                rect: view_rect,
                callback: std::sync::Arc::new(move |_info, render_ctx| unsafe {
                    paint_mesh_callback(render_ctx, &mesh, &proj, time);
                }),
            };
            painter.add(callback);
        }

        response
    }
}
thread_local! {
    pub static MODEL: [f32; 16] = (glam::Mat4::from_rotation_y(90_f32.to_radians())
    * glam::Mat4::from_rotation_z(180_f32.to_radians()))
    .to_cols_array();
}

unsafe fn paint_mesh_callback(
    render_ctx: &mut dyn std::any::Any,
    mesh: &Mesh,
    projection: &[f32],
    _time: f64,
) {
    if let Some(painter) = render_ctx.downcast_ref::<egui_glow::Painter>() {
        use egui_glow::glow;
        let gl = painter.gl();
        gl.bind_vertex_array(None); // Unbind egui_glow vertex array object

        let assets = assets::instance(gl);

        static CAMERA_ARRAY_BUFFER: OnceCell<NativeBuffer> = OnceCell::new();
        static CAMERA_ELEMENT_BUFFER: OnceCell<NativeBuffer> = OnceCell::new();

        let vertex_buffer = CAMERA_ARRAY_BUFFER.get_or_init(|| gl.create_buffer().unwrap());
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(*vertex_buffer));

        let index_buffer = CAMERA_ELEMENT_BUFFER.get_or_init(|| gl.create_buffer().unwrap());
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(*index_buffer));
        let stride = std::mem::size_of::<Vertex>() as i32;

        let (program, texture) = match mesh.material {
            Material::Track => {
                gl.use_program(Some(assets.track_shader));

                gl.uniform_4_f32_slice(
                    gl.get_uniform_location(assets.track_shader, "lCol")
                        .as_ref(),
                    &Hsva::new(0.5, 1.0, 1.0, 1.0)
                        .to_srgba_premultiplied()
                        .map(|v| v as f32 / 255.0),
                );

                gl.uniform_4_f32_slice(
                    gl.get_uniform_location(assets.track_shader, "rCol")
                        .as_ref(),
                    &Hsva::new(0.0, 1.0, 1.0, 1.0)
                        .to_srgba_premultiplied()
                        .map(|v| v as f32 / 255.0),
                );

                (assets.track_shader, Some(assets.track_texture))
            }
            Material::ChipBT => (assets.chip_shader, Some(assets.bt_chip_texture)),
            Material::ChipFX => (assets.chip_shader, Some(assets.fx_chip_texture)),
            Material::LongBT => (assets.hold_shader, Some(assets.bt_hold_texture)),
            Material::LongFX => (assets.hold_shader, Some(assets.fx_chip_texture)),
            Material::Laser(side) => {
                gl.use_program(Some(assets.laser_shader));
                let color = if side == 0 {
                    Hsva::new(0.5, 1.0, 1.0, 1.0)
                } else {
                    Hsva::new(0.0, 1.0, 1.0, 1.0)
                };

                gl.uniform_3_f32_slice(
                    gl.get_uniform_location(assets.laser_shader, "color")
                        .as_ref(),
                    &color.to_srgb().map(|v| v as f32 / 255.0),
                );
                (assets.laser_shader, Some(assets.laser_texture))
            }
        };

        gl.use_program(Some(program));
        gl.bind_texture(glow::TEXTURE_2D, texture);
        gl.active_texture(glow::TEXTURE0);

        if let Some(pos_loc) = gl.get_attrib_location(program, "position") {
            gl.vertex_attrib_pointer_f32(
                pos_loc,
                2,
                glow::FLOAT,
                false,
                stride,
                offset_of!(Vertex, pos) as i32,
            );
            gl.enable_vertex_attrib_array(pos_loc);
        };
        if let Some(texcoord_loc) = gl.get_attrib_location(program, "texcoord") {
            gl.vertex_attrib_pointer_f32(
                texcoord_loc,
                2,
                glow::FLOAT,
                false,
                stride,
                offset_of!(Vertex, uv) as i32,
            );
            gl.enable_vertex_attrib_array(texcoord_loc);
        }
        if let Some(color_loc) = gl.get_attrib_location(program, "color0") {
            gl.vertex_attrib_pointer_f32(
                color_loc,
                4,
                glow::UNSIGNED_BYTE,
                false,
                stride,
                offset_of!(Vertex, color) as i32,
            );
            gl.enable_vertex_attrib_array(color_loc);
        }

        MODEL.with(|m| {
            gl.uniform_matrix_4_f32_slice(
                gl.get_uniform_location(program, "Model").as_ref(),
                false,
                m,
            );
        });

        gl.uniform_matrix_4_f32_slice(
            gl.get_uniform_location(program, "Projection").as_ref(),
            false,
            projection,
        );

        gl.uniform_1_i32(gl.get_uniform_location(program, "mainTex").as_ref(), 0);

        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(&mesh.mesh.vertices),
            glow::STREAM_DRAW,
        );

        gl.buffer_data_u8_slice(
            glow::ELEMENT_ARRAY_BUFFER,
            bytemuck::cast_slice(&mesh.mesh.indices),
            glow::STREAM_DRAW,
        );
        check_for_gl_error!(gl);

        gl.draw_elements(
            glow::TRIANGLES,
            mesh.mesh.indices.len() as i32,
            glow::UNSIGNED_INT,
            0,
        );
        check_for_gl_error!(gl);
    } else {
        eprintln!("Can't do custom painting because we are not using a glow context");
    }
}

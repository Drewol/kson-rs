use std::{rc::Rc, sync::Arc};

use bytemuck::offset_of;
use eframe::{
    egui::{Sense, Widget},
    epaint::{Color32, Hsva, PaintCallback, Stroke, Vertex},
    glow::{Context, HasContext, NativeBuffer},
};
use egui_glow::check_for_gl_error;
use emath::{pos2, vec2, Rect, Vec2};
use kson::Chart;
use once_cell::sync::OnceCell;
use puffin::{profile_function, profile_scope};

use crate::{assets, chart_camera::ChartCamera, rect_xy_wh};

pub enum Material {
    Track(Color32, Color32),
    ChipBT,
    ChipFX,
    LongBT,
    LongFX,
    Laser(u8),
    Solid(BlendMode),
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

    pub fn add_track(&mut self, laser_colors: &[Color32; 2]) {
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
            material: Material::Track(laser_colors[0], laser_colors[1]),
        });
    }
    pub fn add_mesh(&mut self, mesh: Mesh) {
        self.meshes.push(mesh)
    }

    pub fn add_chart_objects(&mut self, chart: &Chart, tick: f32, laser_colors: &[Color32; 2]) {
        profile_function!();
        let tick_height = -0.05;
        let bottom_margin = -tick * tick_height;

        let min_tick_render = tick as i32 - chart.beat.resolution as i32 * 8;

        let screen = crate::chart_editor::ScreenState {
            beat_res: chart.beat.resolution,
            beats_per_col: u32::MAX, //whole chart in one column
            bottom_margin,           //one of these could maybe scroll the chart
            top_margin: 0.0,
            h: 0.0,
            w: 1.0,
            top: 0.0,
            left_margin: -Self::TRACK_WIDTH,
            tick_height,
            track_width: Self::TRACK_WIDTH,
            x_offset: 0.0,
            x_offset_target: 0.0,
            curve_per_tick: 1.0,
        };
        let uv_rect = Rect {
            min: pos2(0.0, 0.0),
            max: pos2(1.0, 0.0),
        };
        let lane_width = screen.lane_width();
        //bt
        let mut bt_chip_mesh = eframe::epaint::Mesh::with_texture(Default::default());
        let mut bt_hold_mesh = eframe::epaint::Mesh::with_texture(Default::default());
        {
            profile_scope!("BT Components");
            for i in 0..4 {
                for n in &chart.note.bt[i] {
                    if ((n.y + n.l) as i32) < min_tick_render {
                        continue;
                    }

                    if n.l == 0 {
                        let (x, y) = screen.tick_to_pos(n.y);

                        let x = x + i as f32 * lane_width + lane_width + screen.track_width / 2.0;
                        let y = y as f32;
                        let w = screen.track_width as f32 / 6.0;
                        let h = Self::TRACK_LENGH / 100.0;

                        bt_chip_mesh.add_rect_with_uv(
                            rect_xy_wh([x, y, w, h]),
                            uv_rect,
                            Color32::WHITE,
                        );
                    } else {
                        for (x, y, h, _) in screen.interval_to_ranges(n) {
                            let x =
                                x + i as f32 * lane_width + lane_width + screen.track_width / 2.0;
                            let w = screen.track_width as f32 / 6.0;

                            bt_hold_mesh.add_rect_with_uv(
                                rect_xy_wh([x, y, w, h]),
                                uv_rect,
                                Color32::WHITE,
                            );
                        }
                    }
                }
            }
        }

        let mut fx_chip_mesh = eframe::epaint::Mesh::with_texture(Default::default());
        let mut fx_hold_mesh = eframe::epaint::Mesh::with_texture(Default::default());
        //fx
        {
            profile_scope!("FX Components");
            for i in 0..2 {
                for n in &chart.note.fx[i] {
                    if ((n.y + n.l) as i32) < min_tick_render {
                        continue;
                    }

                    if n.l == 0 {
                        let (x, y) = screen.tick_to_pos(n.y);

                        let x = x
                            + (i as f32 * lane_width * 2.0)
                            + screen.track_width / 2.0
                            + lane_width;
                        let w = lane_width * 2.0;
                        let h = Self::TRACK_LENGH / 100.0;

                        fx_chip_mesh.add_rect_with_uv(
                            rect_xy_wh([x, y, w, h]),
                            uv_rect,
                            Color32::WHITE,
                        );
                    } else {
                        for (x, y, h, _) in screen.interval_to_ranges(n) {
                            let x = x
                                + (i as f32 * lane_width * 2.0)
                                + screen.track_width / 2.0
                                + lane_width;
                            let w = lane_width * 2.0;

                            fx_hold_mesh.add_rect_with_uv(
                                rect_xy_wh([x, y, w, h]),
                                uv_rect,
                                Color32::WHITE,
                            );
                        }
                    }
                }
            }
        }
        self.add_mesh(Mesh {
            mesh: fx_hold_mesh,
            material: Material::LongFX,
        });
        self.add_mesh(Mesh {
            mesh: bt_hold_mesh,
            material: Material::LongBT,
        });
        self.add_mesh(Mesh {
            mesh: fx_chip_mesh,
            material: Material::ChipFX,
        });
        self.add_mesh(Mesh {
            mesh: bt_chip_mesh,
            material: Material::ChipBT,
        });
        let mapped_color = laser_colors
            .iter()
            .map(Color32::to_srgba_unmultiplied)
            .map(Hsva::from_srgba_unmultiplied)
            .map(|hsva| Hsva::new(hsva.h, 1.0, 1.0, 1.0))
            .map(Color32::from)
            .collect::<Vec<_>>();
        for (side, lane) in chart.note.laser.iter().enumerate() {
            let mut laser_meshes = Vec::new();

            for section in lane {
                screen
                    .draw_laser_section(section, &mut laser_meshes, mapped_color[side], true)
                    .unwrap();
            }
            self.meshes.append(
                &mut laser_meshes
                    .into_iter()
                    .map(|mesh| Mesh {
                        mesh,
                        material: Material::Laser(side as u8),
                    })
                    .collect::<Vec<_>>(),
            )
        }
    }

    pub fn add_track_overlay(&mut self) {
        let left = -(Self::TRACK_WIDTH / 2.0);
        let right = Self::TRACK_WIDTH / 2.0;

        let mut mesh = eframe::epaint::Mesh::with_texture(Default::default());
        mesh.add_colored_rect(
            Rect::from_x_y_ranges((2.0 * left)..=(right * 2.0), -Self::TRACK_LENGH..=0.0),
            Color32::from_gray(20),
        );
        self.add_mesh(Mesh {
            mesh,
            material: Material::Solid(BlendMode::Min),
        });

        let mut mesh = eframe::epaint::Mesh::with_texture(Default::default());
        mesh.add_colored_rect(
            Rect::from_x_y_ranges(left..=right, -0.002..=0.002),
            Color32::RED,
        );
        self.add_mesh(Mesh {
            mesh,
            material: Material::Solid(BlendMode::Normal),
        });
    }
}

impl Widget for CameraView {
    fn ui(self, ui: &mut eframe::egui::Ui) -> eframe::egui::Response {
        let width = ui.available_size_before_wrap().x.max(self.desired_size.x);
        let height = width / (16.0 / 9.0); //16:9 aspect ratio, potentially allow toggle to 9:16
        let time = ui.ctx().input(|x| x.time);
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
                callback: std::sync::Arc::new(move |render_ctx| unsafe {
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
            Material::Track(lcol, rcol) => {
                gl.use_program(Some(assets.track_shader));

                gl.uniform_4_f32_slice(
                    gl.get_uniform_location(assets.track_shader, "lCol")
                        .as_ref(),
                    &lcol.to_srgba_unmultiplied().map(|v| v as f32 / 255.0),
                );

                gl.uniform_4_f32_slice(
                    gl.get_uniform_location(assets.track_shader, "rCol")
                        .as_ref(),
                    &rcol.to_srgba_unmultiplied().map(|v| v as f32 / 255.0),
                );

                (assets.track_shader, Some(assets.track_texture))
            }
            Material::ChipBT => (assets.chip_shader, Some(assets.bt_chip_texture)),
            Material::ChipFX => (assets.chip_shader, Some(assets.fx_chip_texture)),
            Material::LongBT => (assets.hold_shader, Some(assets.bt_hold_texture)),
            Material::LongFX => (assets.hold_shader, Some(assets.fx_hold_texture)),
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
            Material::Solid(mode) => {
                set_blend_mode(gl, mode);
                (assets.color_mesh_shader, None)
            }
        };

        match mesh.material {
            Material::LongFX | Material::Laser(_) => set_blend_mode(gl, BlendMode::Add),
            _ => {}
        }

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
        gl.uniform_1_i32(gl.get_uniform_location(program, "hitState").as_ref(), 1);
        gl.uniform_1_i32(gl.get_uniform_location(program, "hasSample").as_ref(), 0);
        gl.uniform_1_f32(gl.get_uniform_location(program, "objectGlow").as_ref(), 0.0);

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
        set_blend_mode(gl, BlendMode::Normal);
        check_for_gl_error!(gl);
    } else {
        eprintln!("Can't do custom painting because we are not using a glow context");
    }
}

#[derive(Debug, Copy, Clone)]
pub enum BlendMode {
    Normal,
    Add,
    Min,
}

unsafe fn set_blend_mode(gl: &Arc<Context>, mode: BlendMode) {
    use egui_glow::glow;
    gl.enable(glow::BLEND);
    match mode {
        BlendMode::Normal => gl.blend_func_separate(
            glow::SRC_ALPHA,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ONE,
            glow::ONE,
        ),
        BlendMode::Add => gl.blend_func(glow::ONE, glow::ONE),
        BlendMode::Min => {
            gl.blend_func(glow::ONE, glow::ONE);
            gl.blend_equation(glow::MIN);
        }
    }
}

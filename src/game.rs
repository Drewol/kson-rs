use crate::{
    scene::{Scene, SceneData},
    shaded_mesh::ShadedMesh,
};
use kson::{Chart, Ksh, Vox};
pub struct Game {
    view: ChartView,
    chart: kson::Chart,
    camera_pos: Vec3,
    time: i64,
    duration: i64,
    fx_long_shaders: [ShadedMesh; 2],
    bt_long_shaders: [ShadedMesh; 2],
    fx_chip_shaders: [ShadedMesh; 2],
    laser_shaders: [[ShadedMesh; 2]; 2],
    track_shader: [ShadedMesh; 1],
    bt_chip_shader: [ShadedMesh; 1],
}
struct TrackRenderMeshes {
    fx_hold: CpuMesh,
    fx_hold_active: CpuMesh,
    bt_hold: CpuMesh,
    bt_hold_active: CpuMesh,
    fx_chip: CpuMesh,
    fx_chip_sample: CpuMesh,
    bt_chip: CpuMesh,
    lasers: [CpuMesh; 4],
}
pub struct GameData {
    context: three_d::Context,
    chart: kson::Chart,
    skin_folder: PathBuf,
}

pub fn extend_mesh(a: CpuMesh, b: CpuMesh) -> CpuMesh {
    let CpuMesh {
        mut name,
        mut material_name,
        mut positions,
        mut indices,
        mut normals,
        mut tangents,
        mut uvs,
        mut colors,
    } = a;

    let index_offset = positions.len();

    let CpuMesh {
        name: b_name,
        material_name: b_material_name,
        positions: mut b_positions,
        indices: b_indices,
        normals: b_normals,
        tangents: b_tangents,
        uvs: mut b_uvs,
        colors: mut b_colors,
    } = b;

    let indices = match (indices.into_u32(), b_indices.into_u32()) {
        (None, None) => Indices::None,
        (None, Some(mut b)) => {
            b.iter_mut().for_each(|idx| *idx += index_offset as u32);
            Indices::U32(b)
        }
        (Some(a), None) => Indices::U32(a),
        (Some(mut a), Some(mut b)) => {
            b.iter_mut().for_each(|idx| *idx += index_offset as u32);
            a.append(&mut b);
            Indices::U32(a)
        }
    };
    {
        match &mut positions {
            Positions::F32(a) => a.append(&mut b_positions.into_f32()),
            Positions::F64(a) => a.append(&mut b_positions.into_f64()),
        }
    }

    let uvs: Option<Vec<_>> = Some(uvs.iter().chain(b_uvs.iter()).flatten().copied().collect());

    let mut res = CpuMesh {
        name,
        material_name,
        positions,
        indices,
        normals,
        tangents,
        uvs,
        colors,
    };

    res.compute_normals();
    res.compute_tangents();

    res
}

impl GameData {
    pub fn new(
        context: three_d::Context,
        chart: kson::Chart,
        skin_folder: PathBuf,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            context,
            chart,
            skin_folder,
        })
    }
}

impl SceneData for GameData {
    fn make_scene(self: Box<Self>) -> Box<dyn Scene> {
        let Self {
            context,
            chart,
            skin_folder,
        } = *self;

        let mut shader_folder = skin_folder.clone();
        let mut texture_folder = skin_folder.clone();
        shader_folder.push("shaders");
        texture_folder.push("textures");
        texture_folder.push("dummy.png");

        let mut fx_long_shader = ShadedMesh::new(&context, "holdbutton", &shader_folder)
            .expect("Failed to load shader:");
        let mut fx_long_shader_active = ShadedMesh::new(&context, "holdbutton", &shader_folder)
            .expect("Failed to load shader:");

        fx_long_shader.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("fxbuttonhold.png"),
            (false, false),
        );
        fx_long_shader_active.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("fxbuttonhold.png"),
            (false, false),
        );

        let mut bt_long_shader = ShadedMesh::new(&context, "holdbutton", &shader_folder)
            .expect("Failed to load shader:");
        let mut bt_long_shader_active = ShadedMesh::new(&context, "holdbutton", &shader_folder)
            .expect("Failed to load shader:");

        bt_long_shader.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("buttonhold.png"),
            (false, false),
        );
        bt_long_shader_active.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("buttonhold.png"),
            (false, false),
        );

        let mut fx_chip_shader =
            ShadedMesh::new(&context, "button", &shader_folder).expect("Failed to load shader:");
        let mut fx_chip_shader_sample =
            ShadedMesh::new(&context, "button", &shader_folder).expect("Failed to load shader:");

        fx_chip_shader.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("fxbutton.png"),
            (false, false),
        );
        fx_chip_shader_sample.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("fxbutton.png"),
            (false, false),
        );

        let mut bt_chip_shader =
            ShadedMesh::new(&context, "button", &shader_folder).expect("Failed to load shader:");

        bt_chip_shader.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("button.png"),
            (false, false),
        );

        let mut track_shader =
            ShadedMesh::new(&context, "track", &shader_folder).expect("Failed to load shader:");
        track_shader.set_data_mesh(
            &context,
            &xz_rect(Vec3::zero(), vec2(1.0, ChartView::TRACK_LENGTH * 2.0)),
        );

        track_shader.set_param("lCol", Color::BLUE.to_vec4());
        track_shader.set_param("rCol", Color::RED.to_vec4());

        track_shader.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("track.png"),
            (false, false),
        );

        let mut laser_left =
            ShadedMesh::new(&context, "laser", &shader_folder).expect("Failed to load shader:");
        let mut laser_left_active =
            ShadedMesh::new(&context, "laser", &shader_folder).expect("Failed to load shader:");

        let mut laser_right =
            ShadedMesh::new(&context, "laser", &shader_folder).expect("Failed to load shader:");
        let mut laser_right_active =
            ShadedMesh::new(&context, "laser", &shader_folder).expect("Failed to load shader:");

        laser_left.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("laser_l.png"),
            (false, true),
        );
        laser_left_active.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("laser_l.png"),
            (false, true),
        );
        laser_right.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("laser_r.png"),
            (false, true),
        );
        laser_right_active.use_texture(
            &context,
            "mainTex",
            texture_folder.with_file_name("laser_r.png"),
            (false, true),
        );

        laser_left.set_blend(Blend::ADD);
        laser_left_active.set_blend(Blend::ADD);
        laser_right.set_blend(Blend::ADD);
        laser_right_active.set_blend(Blend::ADD);

        Box::new(
            Game::new(
                chart,
                &skin_folder,
                &context,
                [fx_long_shader, fx_long_shader_active],
                [bt_long_shader, bt_long_shader_active],
                [fx_chip_shader, fx_chip_shader_sample],
                [
                    [laser_left, laser_left_active],
                    [laser_right, laser_right_active],
                ],
                [track_shader],
                [bt_chip_shader],
            )
            .unwrap(),
        )
    }
}

impl Game {
    pub fn new(
        chart: Chart,
        skin_root: &PathBuf,
        td: &three_d::Context,

        fx_long_shaders: [ShadedMesh; 2],
        bt_long_shaders: [ShadedMesh; 2],
        fx_chip_shaders: [ShadedMesh; 2],
        laser_shaders: [[ShadedMesh; 2]; 2],
        track_shader: [ShadedMesh; 1],
        bt_chip_shader: [ShadedMesh; 1],
    ) -> Result<Self> {
        let mut view = ChartView::new(skin_root, td);
        view.build_laser_meshes(&chart);
        let duration = chart.get_last_tick();
        let duration = chart.tick_to_ms(duration) as i64;
        let mut res = Self {
            chart,
            view,
            duration,
            time: 0,
            camera_pos: vec3(0.0, 1.0, -1.0),
            bt_chip_shader,
            track_shader,
            bt_long_shaders,
            fx_chip_shaders,
            fx_long_shaders,
            laser_shaders,
        };
        res.set_track_uniforms();
        Ok(res)
    }

    fn set_track_uniforms(&mut self) {
        self.track_shader
            .iter_mut()
            .chain(self.fx_long_shaders.iter_mut())
            .chain(self.bt_long_shaders.iter_mut())
            .chain(self.fx_chip_shaders.iter_mut())
            .chain(self.bt_chip_shader.iter_mut())
            .chain(self.laser_shaders.iter_mut().flatten())
            .for_each(|shader| {
                shader.set_param("trackPos", 0.0);
                shader.set_param("trackScale", 1.0);
                shader.set_param("hiddenCutoff", 0.0);
                shader.set_param("hiddenFadeWindow", 100.0);
                shader.set_param("suddenCutoff", 10.0);
                shader.set_param("suddenFadeWindow", 1000.0);
            });

        self.laser_shaders
            .iter_mut()
            .flatten()
            .for_each(|laser| laser.set_param("objectGlow", 1.0));
        self.laser_shaders[0]
            .iter_mut()
            .for_each(|ll| ll.set_param("color", Color::BLUE.to_vec4()));
        self.laser_shaders[1]
            .iter_mut()
            .for_each(|rl| rl.set_param("color", Color::RED.to_vec4()));
    }
}

impl Scene for Game {
    fn closed(&self) -> bool {
        self.time >= self.duration
    }
    fn render_ui(&mut self, dt: f64) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_suspended(&self) -> bool {
        false
    }

    fn debug_ui(&mut self, ctx: &three_d::egui::Context) -> anyhow::Result<()> {
        use three_d::egui::*;
        Window::new("Debug").show(ctx, |ui| {
            let Vector3 {
                mut x,
                mut y,
                mut z,
            } = self.camera_pos;
            ui.add(Slider::new(&mut x, -10.0..=10.0).logarithmic(true));
            ui.add(Slider::new(&mut y, -10.0..=10.0).logarithmic(true));
            ui.add(Slider::new(&mut z, -10.0..=10.0).logarithmic(true));

            self.camera_pos = vec3(x, y, z);

            ui.add(Slider::new(&mut self.time, 0..=self.duration));
            ui.add(Slider::new(&mut self.view.hispeed, 0.001..=2.0));
        });
        Ok(())
    }

    fn render(
        &mut self,
        dt: f64,
        td_context: &three_d::Context,
        target: &mut three_d::RenderTarget,
        viewport: Viewport,
    ) {
        let camera = Camera::new_perspective(
            viewport,
            self.camera_pos,
            Vec3::zero(),
            Vec3::unit_y(),
            Rad(90.0_f32.to_radians()),
            0.01,
            10000.0,
        );
        self.time += dt as i64;
        self.view.cursor = self.time;
        let render_data = self.view.render(&self.chart, td_context);

        self.bt_chip_shader[0].set_data_mesh(td_context, &render_data.bt_chip);
        self.bt_long_shaders[0].set_data_mesh(td_context, &render_data.bt_hold);
        self.bt_long_shaders[1].set_data_mesh(td_context, &render_data.bt_hold_active);

        self.fx_chip_shaders[0].set_data_mesh(td_context, &render_data.fx_chip);
        self.fx_chip_shaders[1].set_data_mesh(td_context, &render_data.fx_chip_sample);
        self.fx_long_shaders[0].set_data_mesh(td_context, &render_data.fx_hold);
        self.fx_long_shaders[1].set_data_mesh(td_context, &render_data.fx_hold_active);

        self.laser_shaders[0][0].set_data_mesh(td_context, &render_data.lasers[0]);
        self.laser_shaders[0][1].set_data_mesh(td_context, &render_data.lasers[1]);
        self.laser_shaders[1][0].set_data_mesh(td_context, &render_data.lasers[2]);
        self.laser_shaders[1][1].set_data_mesh(td_context, &render_data.lasers[3]);

        target.render(
            &camera,
            self.track_shader
                .iter()
                .chain(self.fx_long_shaders.iter())
                .chain(self.bt_long_shaders.iter())
                .chain(self.fx_chip_shaders.iter())
                .chain(self.bt_chip_shader.iter())
                .chain(self.laser_shaders.iter().flatten()),
            &[],
        );
        let axes = three_d::Axes::new(td_context, 0.01, 0.30);
        target.render(&camera, [axes], &[]);
    }

    fn name(&self) -> &str {
        "Game"
    }
}

use std::{collections::HashMap, path::PathBuf, rc::Rc, sync::Arc};

pub struct ChartView {
    pub hispeed: f32,
    pub cursor: i64,
    laser_meshes: [Vec<Vec<GlVertex>>; 2],
    track: CpuMesh,
    pub state: i32,
}

use anyhow::Result;
use three_d::{
    context::Texture, vec2, vec3, Blend, Camera, Color, ColorMaterial, CpuMesh, CpuTexture,
    DepthTest, Gm, Indices, InnerSpace, Matrix4, Mesh, Positions, Rad, RenderStates, Texture2D,
    Vec2, Vec3, Vec4, Vector3, Viewport, Zero,
};

#[derive(Debug)]
#[repr(C)]
struct GlVec3 {
    x: f32,
    y: f32,
    z: f32,
}

#[derive(Debug)]
#[repr(C)]
struct GlVec2 {
    x: f32,
    y: f32,
}
#[derive(Debug)]
#[repr(C)]
struct GlVertex {
    pos: GlVec3,
    uv: GlVec2,
}

impl GlVertex {
    pub fn new(pos: [f32; 3], uv: [f32; 2]) -> Self {
        GlVertex {
            pos: GlVec3 {
                x: pos[0],
                y: pos[1],
                z: pos[2],
            },
            uv: GlVec2 { x: uv[0], y: uv[1] },
        }
    }
}

fn generate_slam_verts(
    vertices: &mut Vec<GlVertex>,
    start: f32,
    end: f32,
    height: f32,
    xoff: f32,
    y: f32,
    w: f32,
    entry: bool,
    exit: bool,
) {
    let x0 = start.min(end) - xoff;
    let x1 = start.max(end) - xoff - w;
    let y0 = y + height;
    let y1 = y;

    vertices.append(&mut vec![
        GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
        GlVertex::new([y0, 0.0, x1], [0.0, 1.0]),
        GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
        GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
        GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
        GlVertex::new([y1, 0.0, x0], [1.0, 0.0]),
    ]);

    //corners
    {
        /*
        a:
        _____
        |\  |
        | \ |
        |__\|

        b:
        _____
        |  /|
        | / |
        |/__|
        */
        //left
        {
            let x1 = x0;
            let x0 = x0 - w;
            if start > end {
                //b <<<<<
                vertices.append(&mut vec![
                    GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
                    GlVertex::new([y0, 0.0, x1], [1.0, 1.0]),
                    GlVertex::new([y1, 0.0, x0], [0.0, 1.0]),
                    GlVertex::new([y0, 0.0, x1], [0.0, 0.0]),
                    GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
                    GlVertex::new([y1, 0.0, x0], [1.0, 0.0]),
                ]);
            } else {
                //a >>>>>
                vertices.append(&mut vec![
                    GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
                    GlVertex::new([y0, 0.0, x1], [0.0, 1.0]),
                    GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
                    GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
                    GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
                    GlVertex::new([y1, 0.0, x0], [0.0, 0.0]),
                ]);
            }
        }
        //right
        {
            let x0 = x1;
            let x1 = x1 + w;
            if start > end {
                //b <<<<<
                vertices.append(&mut vec![
                    GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
                    GlVertex::new([y0, 0.0, x1], [0.0, 1.0]),
                    GlVertex::new([y1, 0.0, x0], [1.0, 1.0]),
                    GlVertex::new([y0, 0.0, x1], [1.0, 0.0]),
                    GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
                    GlVertex::new([y1, 0.0, x0], [0.0, 0.0]),
                ]);
            } else {
                //a >>>>>
                vertices.append(&mut vec![
                    GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
                    GlVertex::new([y0, 0.0, x1], [1.0, 1.0]),
                    GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
                    GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
                    GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
                    GlVertex::new([y1, 0.0, x0], [1.0, 0.0]),
                ]);
            }
        }
    }

    if entry {
        //entry square
        let x0 = start - w - xoff;
        let x1 = start - xoff;
        let y0 = y;
        let y1 = y - height;

        vertices.append(&mut vec![
            GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
            GlVertex::new([y0, 0.0, x1], [1.0, 0.0]),
            GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
            GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
            GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
            GlVertex::new([y1, 0.0, x0], [0.0, 1.0]),
        ]);
    }
    if exit {
        //exit square
        let x0 = end - w - xoff;
        let x1 = end - xoff;
        let y0 = y + height * 2.0;
        let y1 = y + height;
        vertices.append(&mut vec![
            GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
            GlVertex::new([y0, 0.0, x1], [1.0, 0.0]),
            GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
            GlVertex::new([y0, 0.0, x0], [0.0, 0.0]),
            GlVertex::new([y1, 0.0, x1], [1.0, 1.0]),
            GlVertex::new([y1, 0.0, x0], [0.0, 1.0]),
        ]);
    }
}

pub fn xz_rect(center: Vec3, size: Vec2) -> CpuMesh {
    let indices = vec![0u8, 1, 2, 2, 3, 0];
    let halfsize_x = size.x / 2.0;
    let halfsize_z = size.y / 2.0;
    let positions = vec![
        center + Vec3::new(-halfsize_x, 0.0, -halfsize_z),
        center + Vec3::new(halfsize_x, 0.0, -halfsize_z),
        center + Vec3::new(halfsize_x, 0.0, halfsize_z),
        center + Vec3::new(-halfsize_x, 0.0, halfsize_z),
    ];
    let normals = vec![
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, 1.0),
    ];
    let tangents = vec![
        Vec4::new(1.0, 0.0, 0.0, 1.0),
        Vec4::new(1.0, 0.0, 0.0, 1.0),
        Vec4::new(1.0, 0.0, 0.0, 1.0),
        Vec4::new(1.0, 0.0, 0.0, 1.0),
    ];
    let uvs = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
    ];
    CpuMesh {
        name: "square".to_string(),
        indices: Indices::U8(indices),
        positions: Positions::F32(positions),
        normals: Some(normals),
        tangents: Some(tangents),
        uvs: Some(uvs),
        ..Default::default()
    }
}

fn plane_normal(a: Vec3, b: Vec3, c: Vec3) -> Vector3<f32> {
    // Calculate the edge vectors formed by the three points
    let ab = b - a;
    let ac = c - a;

    // Use the cross product to get the normal to the plane
    ab.cross(ac).normalize()
}

fn plane_angle(v1: Vector3<f32>, v2: Vector3<f32>, normal: Vector3<f32>) -> f32 {
    // Project the vectors onto the plane
    let v1_on_plane = v1 - (v1.dot(normal) / normal.dot(normal)) * normal;
    let v2_on_plane = v2 - (v2.dot(normal) / normal.dot(normal)) * normal;

    // Calculate the angle between the vectors on the plane
    let dot = v1_on_plane.dot(v2_on_plane);
    let mag = v1_on_plane.magnitude() * v2_on_plane.magnitude();
    (dot / mag).acos()
}

fn draw_line_3d(a: Vec3, b: Vec3, r: f32) -> CpuMesh {
    let mut mesh = CpuMesh::cylinder(8);

    let line_vector = b - a;
    let line_length = line_vector.magnitude();
    let line_direction = line_vector.normalize();

    let rotation_axis = plane_normal(line_direction, Vec3::unit_x(), Vec3::zero());

    //vector difference should make up a plane and rotating along the normal should work?

    let trans = Matrix4::from_translation(a)
        * Matrix4::from_axis_angle(
            rotation_axis,
            Rad(plane_angle(line_direction, Vec3::unit_x(), rotation_axis)),
        )
        * Matrix4::from_nonuniform_scale(line_length, r, r);
    mesh.transform(&trans);

    mesh
}

fn draw_plane(center: Vec3, size: Vec2, normal: Vec3) -> CpuMesh {
    let mut square = CpuMesh::square();
    let plane_matrix = [
        [size.x, 0.0, 0.0, 0.0],
        [0.0, size.y, 0.0, 0.0],
        [normal.x, normal.y, normal.z, 0.0],
        [center.x, center.y, center.z, 1.0],
    ];

    square.transform(&Matrix4::from_cols(
        plane_matrix[0].into(),
        plane_matrix[1].into(),
        plane_matrix[2].into(),
        plane_matrix[3].into(),
    ));
    square
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let h = h % 1.0; // wrap hue value around 1.0
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;

    let (r, g, b) = match h {
        h if h < 0.16666666666666666 => (c, x, 0.0),
        h if h < 0.3333333333333333 => (x, c, 0.0),
        h if h < 0.5 => (0.0, c, x),
        h if h < 0.6666666666666666 => (0.0, x, c),
        h if h < 0.8333333333333334 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    [r + m, g + m, b + m]
}

impl ChartView {
    pub const TRACK_LENGTH: f32 = 12.0;

    pub fn new(skin_root: &PathBuf, td: &three_d::Context) -> Self {
        let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
        let mut texure_path = skin_root.clone();
        texure_path.push("textures");
        texure_path.push("file.png");
        td.set_depth_test(three_d::DepthTest::Never);

        let mut textures = three_d_asset::io::load(&[
            texure_path.with_file_name("laser_l.png"),
            texure_path.with_file_name("laser_r.png"),
            texure_path.with_file_name("track.png"),
            texure_path.with_file_name("fxbutton.png"),
            texure_path.with_file_name("button.png"),
        ])
        .unwrap();

        let laser_texture = Some(Arc::new(Texture2D::new(
            td,
            &textures.deserialize("laser_l").unwrap(),
        )));
        let laser_render_states = RenderStates {
            blend: Blend::ADD,
            depth_test: DepthTest::Always,
            ..Default::default()
        };

        let track_texture = Arc::new(Texture2D::new(td, &textures.deserialize("track").unwrap()));

        let track_mat = Rc::new(ColorMaterial {
            color: Color::WHITE,
            texture: Some(track_texture),
            render_states: RenderStates {
                depth_test: three_d::DepthTest::Always,
                ..Default::default()
            },
            ..Default::default()
        });

        let track = xz_rect(vec3(0.0, 0.0, 0.0), vec2(1.0, Self::TRACK_LENGTH * 2.0));
        let button_render_states = RenderStates {
            depth_test: DepthTest::Always,
            ..Default::default()
        };

        ChartView {
            cursor: 0,
            hispeed: 1.0,
            laser_meshes: [Vec::new(), Vec::new()],
            track,
            state: 0,
        }
    }

    pub fn build_laser_meshes(&mut self, chart: &kson::Chart) {
        for i in 0..2 {
            self.laser_meshes[i].clear();
            for section in &chart.note.laser[i] {
                let mut section_verts = Vec::new();
                let w = 1.0 / 6.0;
                let (xoff, track_w) = if section.wide() < 2 {
                    (2.0 / 6.0, 5.0 / 6.0)
                } else {
                    (2.0 / 6.0, 11.0 / 12.0)
                };
                let mut is_first = true;
                for se in section.segments() {
                    let s = se[0];
                    let e = se[1];
                    let mut syoff = 0.0 as f32;
                    let mut start_value = s.v as f32 * track_w;

                    if let Some(value) = s.vf {
                        let value = value as f32 * track_w;
                        syoff = chart.beat.resolution as f32 / 8.0;
                        generate_slam_verts(
                            &mut section_verts,
                            start_value,
                            value,
                            syoff,
                            xoff,
                            s.ry as f32,
                            w,
                            is_first,
                            false,
                        );
                        start_value = value as f32;
                    }
                    let end_value = e.v as f32 * track_w;
                    let x00 = end_value - w - xoff;
                    let x01 = end_value - xoff;
                    let x10 = start_value - w - xoff;
                    let x11 = start_value - xoff;
                    let y0 = e.ry as f32;
                    let y1 = s.ry as f32 + syoff;

                    section_verts.append(&mut vec![
                        GlVertex::new([y0, 0.0, x00], [0.0, 0.0]),
                        GlVertex::new([y0, 0.0, x01], [1.0, 0.0]),
                        GlVertex::new([y1, 0.0, x11], [1.0, 1.0]),
                        GlVertex::new([y0, 0.0, x00], [0.0, 0.0]),
                        GlVertex::new([y1, 0.0, x10], [0.0, 1.0]),
                        GlVertex::new([y1, 0.0, x11], [1.0, 1.0]),
                    ]);
                    is_first = false;
                }
                if let Some(e) = section.last() {
                    if let Some(value) = e.vf {
                        let start_value = e.v as f32 * track_w;
                        let value = value as f32 * track_w;
                        let syoff = chart.beat.resolution as f32 / 8.0;
                        generate_slam_verts(
                            &mut section_verts,
                            start_value,
                            value,
                            syoff,
                            xoff,
                            e.ry as f32,
                            w,
                            is_first,
                            true,
                        );
                    }
                }
                self.laser_meshes[i].push(section_verts);
            }
        }
    }

    fn render(&mut self, chart: &kson::Chart, td: &three_d::Context) -> TrackRenderMeshes {
        use three_d::prelude::*;
        let view_time = self.cursor - chart.audio.clone().bgm.unwrap().offset as i64;
        let view_offset = if view_time < 0 {
            chart.ms_to_tick(view_time.abs() as f64) as i64 //will be weird with early bpm changes
        } else {
            0
        };

        td.set_depth_test(three_d::DepthTest::Never);

        let glow_state = if (0.0_f32 * 8.0).fract() > 0.5 { 2 } else { 3 };
        let view_tick = chart.ms_to_tick(view_time as f64) as i64 - view_offset;
        let view_distance = (chart.beat.resolution as f32 * 4.0) / self.hispeed;
        let last_view_tick = view_distance.ceil() as i64 + view_tick;
        let first_view_tick = view_tick - view_distance as i64;
        let y_view_div = ((chart.beat.resolution as f32 * 4.0) / self.hispeed) / Self::TRACK_LENGTH;
        let white_mat = Rc::new(ColorMaterial {
            color: Color::WHITE,
            ..Default::default()
        });

        #[derive(Debug, PartialEq, Eq, Clone, Copy)]
        enum NoteType {
            BtChip,
            BtHold,
            BtHoldActive,
            FxChip,
            FxChipSample,
            FxHold,
            FxHoldActive,
        }
        let mut notes = Vec::new();
        let chip_h = 0.05;

        let track = self.track.clone();

        for i in 0..4 {
            for n in &chart.note.bt[i] {
                if (n.y as i64) > last_view_tick {
                    break;
                } else if ((n.y + n.l) as i64) < first_view_tick {
                    continue;
                }

                let w = 0.9 / 6.0;
                let x = 1.5 / 6.0 + (i as f32 / 6.0);
                let h = if n.l == 0 {
                    chip_h
                } else {
                    (n.l as f32) / y_view_div
                };
                let yoff = (view_tick as i64 - n.y as i64) as f32;
                let y = yoff / y_view_div - h;
                let p = if n.l == 0 { 2 } else { 1 }; //sorting priority
                notes.push((
                    vec3(x, 0.0, y),
                    vec2(w, h),
                    if n.l > 0 {
                        NoteType::BtHold
                    } else {
                        NoteType::BtChip
                    },
                ));
            }
        }
        for i in 0..2 {
            for n in &chart.note.fx[i] {
                if (n.y as i64) > last_view_tick {
                    break;
                } else if ((n.y + n.l) as i64) < first_view_tick {
                    continue;
                }
                let w = 1.0 / 3.0;
                let x = 1.0 / 3.0 + (1.0 / 3.0) * i as f32;
                let h = if n.l == 0 {
                    chip_h
                } else {
                    (n.l as f32) / y_view_div
                };
                let yoff = (view_tick as i64 - n.y as i64) as f32;
                let y = yoff / y_view_div - h;
                let p = if n.l == 0 { 3 } else { 0 }; //sorting priority
                notes.push((
                    vec3(x, 0.0, y),
                    vec2(w, h),
                    if n.l > 0 {
                        NoteType::FxHold
                    } else {
                        NoteType::FxChip
                    },
                ));
            }
        }

        let notes = notes
            .iter()
            .map(|n| (xz_rect(n.0 - vec3(0.5, 0.0, n.1.y / -2.0), n.1), n.2));

        let mut fx_hold = xz_rect(Vec3::zero(), Vec2::zero());
        let mut fx_hold_active = xz_rect(Vec3::zero(), Vec2::zero());
        let mut bt_hold = xz_rect(Vec3::zero(), Vec2::zero());
        let mut bt_hold_active = xz_rect(Vec3::zero(), Vec2::zero());
        let mut fx_chip = xz_rect(Vec3::zero(), Vec2::zero());
        let mut fx_chip_sample = xz_rect(Vec3::zero(), Vec2::zero());
        let mut bt_chip = xz_rect(Vec3::zero(), Vec2::zero());
        let mut lasers = [
            xz_rect(Vec3::zero(), Vec2::zero()),
            xz_rect(Vec3::zero(), Vec2::zero()),
            xz_rect(Vec3::zero(), Vec2::zero()),
            xz_rect(Vec3::zero(), Vec2::zero()),
        ];

        for n in notes {
            match n.1 {
                NoteType::BtChip => bt_chip = extend_mesh(bt_chip, n.0),
                NoteType::BtHold => bt_hold = extend_mesh(bt_hold, n.0),
                NoteType::BtHoldActive => bt_hold_active = extend_mesh(bt_hold_active, n.0),
                NoteType::FxChip => fx_chip = extend_mesh(fx_chip, n.0),
                NoteType::FxChipSample => fx_chip_sample = extend_mesh(fx_chip_sample, n.0),
                NoteType::FxHold => fx_hold = extend_mesh(fx_hold, n.0),
                NoteType::FxHoldActive => fx_hold_active = extend_mesh(fx_hold_active, n.0),
            }
        }

        //lasers
        {
            for i in 0..2 {
                for (sidx, s) in chart.note.laser[i].iter().enumerate() {
                    let end_y = s.tick() + s.last().unwrap().ry;
                    if (s.tick() as i64) > last_view_tick {
                        break;
                    } else if (end_y as i64) < first_view_tick {
                        continue;
                    }
                    let vertices = self.laser_meshes[i].get(sidx).unwrap();
                    let yoff = (view_tick as i64 - s.tick() as i64) as f32;
                    let laser_mesh = CpuMesh {
                        indices: Indices::U32((0u32..(vertices.len() as u32)).collect()),
                        positions: three_d::Positions::F32(
                            vertices
                                .iter()
                                .map(|v| vec3(v.pos.z, v.pos.y, (yoff - v.pos.x) / y_view_div))
                                .collect(),
                        ),
                        uvs: Some(vertices.iter().map(|v| vec2(v.uv.x, v.uv.y)).collect()),
                        ..Default::default()
                    };

                    let active = 0;
                    let extending = std::mem::take(&mut lasers[i * 2 + active]);
                    let extended = extend_mesh(extending, laser_mesh);
                    lasers[i * 2 + active] = extended;
                }
            }
        }
        TrackRenderMeshes {
            fx_hold,
            fx_hold_active,
            bt_hold,
            bt_hold_active,
            fx_chip,
            fx_chip_sample,
            bt_chip,
            lasers,
        }
    }
}

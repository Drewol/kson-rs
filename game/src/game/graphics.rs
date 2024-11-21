use three_d::{prelude::*, vec2, vec3, Camera, CpuMesh, Indices, Positions, Vec2, Vec3, Vector3};

use three_d_asset::Srgba;

use super::HoldState;

use three_d::Mat4;

pub(crate) struct TrackRenderMeshes {
    pub(crate) fx_hold: Vec<(Mat4, HoldState)>,
    pub(crate) bt_hold: Vec<(Mat4, HoldState)>,
    pub(crate) fx_chip: Vec<(Mat4, bool)>,
    pub(crate) bt_chip: Vec<Mat4>,
    pub(crate) lasers: [CpuMesh; 4],
    pub(crate) lane_beams: [(Mat4, Srgba); 6],
}

pub fn extend_mesh(a: CpuMesh, b: CpuMesh) -> CpuMesh {
    let CpuMesh {
        mut positions,
        indices,
        normals,
        tangents,
        uvs,
        mut colors,
    } = a;

    let index_offset = positions.len();

    let CpuMesh {
        positions: b_positions,
        indices: b_indices,
        normals: _b_normals,
        tangents: _b_tangents,
        uvs: b_uvs,
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

    if let (Some(a), Some(b)) = (colors.as_mut(), b_colors.as_mut()) {
        a.append(b)
    } else {
        colors = None;
    }

    let uvs: Option<Vec<_>> = Some(uvs.iter().chain(b_uvs.iter()).flatten().copied().collect());

    let mut res = CpuMesh {
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

#[derive(Debug)]
#[repr(C)]
pub(crate) struct GlVec3 {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) z: f32,
}

#[derive(Debug)]
#[repr(C)]
pub(crate) struct GlVec2 {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

#[derive(Debug)]
#[repr(C)]
pub(crate) struct GlVertex {
    pub(crate) pos: GlVec3,
    pub(crate) uv: GlVec2,
}

impl GlVertex {
    pub const fn new(pos: [f32; 3], uv: [f32; 2]) -> Self {
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

pub(crate) fn generate_slam_verts(
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
        let y0 = height.mul_add(2.0, y);
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

pub fn xy_rect(center: Vec3, size: Vec2) -> CpuMesh {
    let indices = vec![0u8, 1, 2, 2, 3, 0];
    let halfsize_x = size.x / 2.0;
    let halfsize_z = size.y / 2.0;
    let positions = vec![
        center + Vec3::new(-halfsize_x, -halfsize_z, 0.0),
        center + Vec3::new(halfsize_x, -halfsize_z, 0.0),
        center + Vec3::new(halfsize_x, halfsize_z, 0.0),
        center + Vec3::new(-halfsize_x, halfsize_z, 0.0),
    ];

    let uvs = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
    ];
    CpuMesh {
        indices: Indices::U8(indices),
        positions: Positions::F32(positions),
        uvs: Some(uvs),
        ..Default::default()
    }
}

pub(crate) fn camera_to_screen(camera: &Camera, point: Vec3, screen: Vec2) -> Vec2 {
    let Vector3 { x, y, z } = point;
    let camera_space = camera.view().transform_point(three_d::Point3 { x, y, z });
    let mut screen_space = camera.projection().transform_point(camera_space);
    screen_space.y = -screen_space.y;
    screen_space *= 0.5f32;
    screen_space += vec3(0.5, 0.5, 0.5);
    vec2(screen_space.x * screen.x, screen_space.y * screen.y)
}

use glam::{vec2, Vec2, Vec3, Vec4};
use three_d_asset::Transform;

#[derive(Default)]
pub(crate) struct TrackRenderMeshes {
    pub(crate) fx_hold: CpuMesh,
    pub(crate) bt_hold: CpuMesh,
    pub(crate) fx_chip: CpuMesh,
    pub(crate) bt_chip: CpuMesh,
    pub(crate) lasers: [CpuMesh; 4],
    pub(crate) lane_beams: CpuMesh,
}

#[derive(Debug, Default, Clone)]
pub struct CpuMesh {
    pub positions: Vec<Vec3>,
    pub indices: Option<Vec<u32>>,
    pub normals: Option<Vec<Vec3>>,
    pub tangents: Option<Vec<Vec3>>,
    pub uvs: Option<Vec<Vec2>>,
    pub colors: Option<Vec<Vec4>>,
}

impl CpuMesh {
    pub fn clear(&mut self) {
        let CpuMesh {
            positions,
            indices,
            normals,
            tangents,
            uvs,
            colors,
        } = self;
        positions.clear();
        indices.as_mut().map(|x| x.clear());
        normals.as_mut().map(|x| x.clear());
        tangents.as_mut().map(|x| x.clear());
        uvs.as_mut().map(|x| x.clear());
        colors.as_mut().map(|x| x.clear());
    }

    pub fn extend(&mut self, other: CpuMesh) {
        *self = extend_mesh(std::mem::take(self), other)
    }
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
        positions: mut b_positions,
        indices: b_indices,
        normals: _b_normals,
        tangents: _b_tangents,
        uvs: b_uvs,
        colors: mut b_colors,
    } = b;

    let indices = match (indices, b_indices) {
        (None, None) => None,
        (None, Some(mut b)) => {
            b.iter_mut().for_each(|idx| *idx += index_offset as u32);
            Some(b)
        }
        (Some(a), None) => Some(a),
        (Some(mut a), Some(mut b)) => {
            b.iter_mut().for_each(|idx| *idx += index_offset as u32);
            a.append(&mut b);
            Some(a)
        }
    };
    {
        positions.append(&mut b_positions);
    }

    if let (Some(a), Some(b)) = (colors.as_mut(), b_colors.as_mut()) {
        a.append(b)
    } else {
        colors = None;
    }

    let uvs: Option<Vec<_>> = Some(uvs.iter().chain(b_uvs.iter()).flatten().copied().collect());
    CpuMesh {
        positions,
        indices,
        normals,
        tangents,
        uvs,
        colors,
    }
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

pub fn xy_rect(center: Vec3, size: Vec2) -> CpuMesh {
    let indices = vec![0, 1, 2, 2, 3, 0];
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
        indices: Some(indices),
        positions: positions,
        uvs: Some(uvs),
        ..Default::default()
    }
}

pub(crate) fn camera_to_screen(camera: &three_d_asset::Camera, point: Vec3, screen: Vec2) -> Vec2 {
    let Vec3 { x, y, z } = point;
    let camera_space = camera
        .view()
        .transform_point(three_d_asset::Point3 { x, y, z });
    let mut screen_space = camera.projection().transform_point(camera_space);
    screen_space.y = -screen_space.y;
    screen_space *= 0.5f32;
    screen_space += three_d_asset::vec3(0.5, 0.5, 0.5);
    vec2(screen_space.x * screen.x, screen_space.y * screen.y)
}

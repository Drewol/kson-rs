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

    /// Appends `other` to this mesh in place.
    pub fn extend(&mut self, other: CpuMesh) {
        let index_offset = self.positions.len() as u32;
        let other_len = other.positions.len();

        match (self.indices.as_mut(), other.indices) {
            (Some(a), Some(b)) => a.extend(b.into_iter().map(|i| i + index_offset)),
            (Some(a), None) => a.extend(index_offset..index_offset + other_len as u32),
            (None, Some(b)) => {
                let mut indices: Vec<u32> = (0..index_offset).collect();
                indices.extend(b.into_iter().map(|i| i + index_offset));
                self.indices = Some(indices);
            }
            (None, None) => {}
        }

        match (self.uvs.as_mut(), other.uvs) {
            (Some(a), Some(b)) => a.extend(b),
            (Some(a), None) => a.extend(std::iter::repeat_n(Vec2::ZERO, other_len)),
            (None, Some(b)) => {
                let mut uvs = vec![Vec2::ZERO; self.positions.len()];
                uvs.extend(b);
                self.uvs = Some(uvs);
            }
            (None, None) => {}
        }

        match (self.colors.as_mut(), other.colors) {
            (Some(a), Some(b)) => a.extend(b),
            (Some(a), None) => a.extend(std::iter::repeat_n(Vec4::ONE, other_len)),
            (None, Some(b)) => {
                let mut colors = vec![Vec4::ONE; self.positions.len()];
                colors.extend(b);
                self.colors = Some(colors);
            }
            (None, None) => {}
        }

        self.positions.extend(other.positions);
    }

    /// Appends `base` transformed by `transform` with every vertex set to `color`.
    pub fn push_transformed(&mut self, base: &CpuMesh, transform: glam::Mat4, color: Vec4) {
        let index_offset = self.positions.len() as u32;
        let base_len = base.positions.len();

        let indices = self.indices.get_or_insert_with(Vec::new);
        match base.indices.as_ref() {
            Some(b) => indices.extend(b.iter().map(|i| i + index_offset)),
            None => indices.extend(index_offset..index_offset + base_len as u32),
        }

        let uvs = self.uvs.get_or_insert_with(Vec::new);
        match base.uvs.as_ref() {
            Some(b) => uvs.extend_from_slice(b),
            None => uvs.extend(std::iter::repeat_n(Vec2::ZERO, base_len)),
        }

        self.colors
            .get_or_insert_with(Vec::new)
            .extend(std::iter::repeat_n(color, base_len));

        self.positions
            .extend(base.positions.iter().map(|p| transform.transform_point3(*p)));
    }
}

pub fn extend_mesh(mut a: CpuMesh, b: CpuMesh) -> CpuMesh {
    a.extend(b);
    a
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

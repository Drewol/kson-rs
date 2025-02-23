use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, RwLock},
};

use anyhow::ensure;
use di::RefMut;
use itertools::Itertools;
use puffin::profile_function;
use tealr::{
    mlu::{
        mlua::{self, FromLua, Lua},
        TealData, UserData,
    },
    ToTypename,
};
use three_d::{
    vec2, vec3, vec4, AxisAlignedBoundingBox, Blend, BufferDataType, Context, CpuTexture,
    ElementBuffer, ElementBufferDataType, Geometry, Mat4, Object, Program, RenderStates,
    SquareMatrix, Texture2D, Vec2, Vec3, Vec4, VertexBuffer, Wrapping,
};
use three_d_asset::{Srgba, Vector2, Vector3, Vector4};

use crate::{config::GameConfig, game_data, help::transform_shader, vg_ui::Vgfx, FrameInput};

pub enum ShaderParam {
    Int(i32),
    Single(f32),
    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),
    IVec2(Vector2<i32>),
    IVec3(Vector3<i32>),
    IVec4(Vector4<i32>),
    Texture(Texture2D),
}

impl From<Vector2<i32>> for ShaderParam {
    fn from(value: Vector2<i32>) -> Self {
        Self::IVec2(value)
    }
}

impl From<Vector3<i32>> for ShaderParam {
    fn from(value: Vector3<i32>) -> Self {
        Self::IVec3(value)
    }
}

impl From<Vector4<i32>> for ShaderParam {
    fn from(value: Vector4<i32>) -> Self {
        Self::IVec4(value)
    }
}

impl From<f32> for ShaderParam {
    fn from(value: f32) -> Self {
        Self::Single(value)
    }
}

impl From<Vec2> for ShaderParam {
    fn from(value: Vec2) -> Self {
        Self::Vec2(value)
    }
}
impl From<Vec3> for ShaderParam {
    fn from(value: Vec3) -> Self {
        Self::Vec3(value)
    }
}
impl From<Vec4> for ShaderParam {
    fn from(value: Vec4) -> Self {
        Self::Vec4(value)
    }
}
impl From<Texture2D> for ShaderParam {
    fn from(value: Texture2D) -> Self {
        Self::Texture(value)
    }
}

impl From<i32> for ShaderParam {
    fn from(value: i32) -> Self {
        Self::Int(value)
    }
}

impl From<Srgba> for ShaderParam {
    fn from(value: Srgba) -> Self {
        Self::Vec4(value.into())
    }
}

/// https://www.khronos.org/opengl/wiki/Primitive#Triangle_primitives for calculating indecies
enum DrawingMode {
    Triangles = 0,
    Fan = 1,
    Strip = 2,
}

//TODO: Cloneable with Arc for gpu resources for better shader reuse
#[derive(UserData, ToTypename)]
pub struct ShadedMesh {
    params: HashMap<String, ShaderParam>,
    material: three_d::Program,
    state: RenderStates,
    vertex_count: usize,
    draw_mode: DrawingMode,
    indecies: ElementBuffer<u32>,
    vertecies_pos: VertexBuffer<Vector3<f32>>,
    vertecies_uv: VertexBuffer<Vector2<f32>>,
    vertecies_color: Option<VertexBuffer<Vector4<f32>>>,
    aabb: AxisAlignedBoundingBox,
    transform: Mat4,
    context: Context,
    requires_in_tex: bool,
    requires_in_color: bool,
}

fn flip_image_data<T>(d: &mut [T], width: usize, height: usize) {
    for y in 0..(height / 2) {
        for x in 0..width {
            d.swap(y * width + x, (height - y - 1) * width + x);
        }
    }
}

impl ShadedMesh {
    pub fn new(
        context: &three_d::Context,
        material: &str,
        path: impl AsRef<Path>,
    ) -> anyhow::Result<Self> {
        let mut shader_path = path.as_ref().to_path_buf();
        shader_path.push(material);
        shader_path.set_extension("vs");
        profile_function!();

        let vertex_shader_source = transform_shader(std::fs::read_to_string(&shader_path)?);

        let fragment_shader_source =
            transform_shader(std::fs::read_to_string(shader_path.with_extension("fs"))?);

        let mut params = HashMap::new();

        let material =
            Program::from_source(context, &vertex_shader_source, &fragment_shader_source)?;

        if material.requires_uniform("color") {
            params.insert("color".into(), vec4(1.0, 1.0, 1.0, 1.0).into());
        }
        let requires_in_color = material.requires_attribute("inColor");
        let requires_in_tex = material.requires_attribute("inTex");
        Ok(Self {
            params,
            material,
            state: RenderStates {
                cull: three_d::Cull::None,
                blend: Blend::TRANSPARENCY,
                depth_test: three_d::DepthTest::Always,
                write_mask: three_d::WriteMask::COLOR,
            },
            vertex_count: 0,
            draw_mode: DrawingMode::Triangles,
            indecies: ElementBuffer::new(context),
            vertecies_pos: VertexBuffer::new(context),
            vertecies_uv: VertexBuffer::new(context),
            aabb: AxisAlignedBoundingBox::EMPTY,
            transform: Mat4::identity(),
            vertecies_color: None,
            context: context.clone(),
            requires_in_color,
            requires_in_tex,
        })
    }

    pub fn new_fullscreen(
        context: &three_d::Context,
        fragment_shader: &str,
    ) -> anyhow::Result<Self> {
        let vertecies_pos = VertexBuffer::new_with_data(
            context,
            &(0..3).map(|x| vec3(x as f32, 0.0, 0.0)).collect_vec(),
        );

        Ok(Self {
            params: HashMap::default(),
            material: Program::from_source(
                //https://stackoverflow.com/questions/2588875/whats-the-best-way-to-draw-a-fullscreen-quad-in-opengl-3-2
                context,
                include_str!("static_assets/fullscreen.vs"),
                fragment_shader,
            )?,
            state: RenderStates {
                cull: three_d::Cull::None,
                blend: Blend::TRANSPARENCY,
                depth_test: three_d::DepthTest::Always,
                write_mask: three_d::WriteMask::COLOR,
            },
            vertex_count: 0,
            draw_mode: DrawingMode::Triangles,
            indecies: ElementBuffer::new(context),
            vertecies_pos,
            vertecies_uv: VertexBuffer::new(context),
            aabb: AxisAlignedBoundingBox::EMPTY,
            transform: Mat4::identity(),
            vertecies_color: None,
            context: context.clone(),
            requires_in_color: false,
            requires_in_tex: false,
        })
    }

    pub fn with_transform(mut self, transform: Mat4) -> Self {
        self.transform = transform;
        self
    }

    pub fn set_blend(&mut self, blend: Blend) {
        self.state.blend = blend;
    }

    fn update_indecies(&mut self) -> anyhow::Result<()> {
        profile_function!();
        let vertex_multiple = match self.draw_mode {
            DrawingMode::Triangles => 3,
            DrawingMode::Strip => 1,
            DrawingMode::Fan => 1,
        };

        ensure!(
            (self.vertex_count % vertex_multiple) == 0,
            "Vertex count not a multiple of {}",
            vertex_multiple
        );

        let index_list: Vec<u32> = (0u32..self.vertex_count as u32).collect();

        let indecies = match self.draw_mode {
            DrawingMode::Triangles => index_list,
            DrawingMode::Strip => index_list
                .windows(3)
                .enumerate()
                .flat_map(|(i, ids)| {
                    if i % 2 == 0 {
                        [ids[0], ids[1], ids[2]]
                    } else {
                        [ids[1], ids[0], ids[2]]
                    }
                })
                .collect(),
            DrawingMode::Fan => index_list
                .windows(2)
                .skip(1)
                .flat_map(|x| [0, x[0], x[1]])
                .collect(),
        };

        self.indecies.fill(&indecies);
        Ok(())
    }

    pub fn set_param(&mut self, key: &str, param: impl Into<ShaderParam>) {
        if self.material.requires_uniform(key) {
            let key: String = key.into();
            self.params.insert(key, param.into());
        }
    }

    pub fn use_texture(
        &mut self,
        name: impl Into<String>,
        path: impl AsRef<Path>,
        wrap_xy: (bool, bool),
        flip_y: bool,
    ) -> anyhow::Result<CpuTexture> {
        profile_function!();
        let name = name.into();
        let mut cpu_texture: CpuTexture = three_d_asset::io::load_and_deserialize(path)?;

        log::info!("{}", &cpu_texture.name);
        cpu_texture.data = match cpu_texture.data {
            three_d::TextureData::RU8(luma) => {
                three_d::TextureData::RgbaU8(luma.into_iter().map(|v| [v, v, v, 255u8]).collect())
            }
            three_d::TextureData::RgU8(luma_alpha) => three_d::TextureData::RgbaU8(
                luma_alpha
                    .into_iter()
                    .map(|la| [la[0], la[0], la[0], la[1]])
                    .collect(),
            ),

            data => data,
        };

        if flip_y {
            let width = cpu_texture.width as usize;
            let height = cpu_texture.height as usize;
            match &mut cpu_texture.data {
                three_d::TextureData::RU8(vec) => flip_image_data(vec, width, height),
                three_d::TextureData::RgU8(vec) => flip_image_data(vec, width, height),
                three_d::TextureData::RgbU8(vec) => flip_image_data(vec, width, height),
                three_d::TextureData::RgbaU8(vec) => flip_image_data(vec, width, height),
                three_d::TextureData::RF16(vec) => flip_image_data(vec, width, height),
                three_d::TextureData::RgF16(vec) => flip_image_data(vec, width, height),
                three_d::TextureData::RgbF16(vec) => flip_image_data(vec, width, height),
                three_d::TextureData::RgbaF16(vec) => flip_image_data(vec, width, height),
                three_d::TextureData::RF32(vec) => flip_image_data(vec, width, height),
                three_d::TextureData::RgF32(vec) => flip_image_data(vec, width, height),
                three_d::TextureData::RgbF32(vec) => flip_image_data(vec, width, height),
                three_d::TextureData::RgbaF32(vec) => flip_image_data(vec, width, height),
            }
        }

        match wrap_xy {
            (true, true) => {
                cpu_texture.wrap_s = Wrapping::Repeat;
                cpu_texture.wrap_t = Wrapping::Repeat;
            }
            (true, false) => {
                cpu_texture.wrap_s = Wrapping::Repeat;
                cpu_texture.wrap_t = Wrapping::ClampToEdge;
            }
            (false, true) => {
                cpu_texture.wrap_s = Wrapping::ClampToEdge;
                cpu_texture.wrap_t = Wrapping::Repeat;
            }
            (false, false) => {
                cpu_texture.wrap_s = Wrapping::ClampToEdge;
                cpu_texture.wrap_t = Wrapping::ClampToEdge;
            }
        }

        let texture = three_d::Texture2D::new(&self.context, &cpu_texture);

        if self.material.requires_uniform(&name) {
            self.material.use_texture(&name, &texture);
            self.params.insert(name, ShaderParam::Texture(texture));
        }
        Ok(cpu_texture)
    }

    #[inline]
    fn use_params(&self) {
        for (name, param) in &self.params {
            match param {
                ShaderParam::Single(v) => self.material.use_uniform(name, v),
                ShaderParam::Vec2(v) => self.material.use_uniform(name, v),
                ShaderParam::Vec3(v) => self.material.use_uniform(name, v),
                ShaderParam::Vec4(v) => self.material.use_uniform(name, v),
                ShaderParam::IVec2(v) => self.material.use_uniform(name, v),
                ShaderParam::IVec3(v) => self.material.use_uniform(name, v),
                ShaderParam::IVec4(v) => self.material.use_uniform(name, v),
                ShaderParam::Int(v) => self.material.use_uniform(name, v),
                ShaderParam::Texture(v) => self.material.use_texture(name, v),
            }
        }
    }

    #[allow(unused)]
    pub fn draw(&self, frame: &FrameInput) -> Result<(), tealr::mlu::mlua::Error> {
        self.use_params();
        self.material
            .use_vertex_attribute("inPos", &self.vertecies_pos);
        if self.requires_in_tex {
            self.material
                .use_vertex_attribute("inTex", &self.vertecies_uv); //UVs
        }

        if let Some(colors) = self.vertecies_color.as_ref() {
            if self.requires_in_color {
                self.material.use_vertex_attribute("inColor", colors);
            }
        }

        self.material
            .draw_elements(self.state, frame.viewport, &self.indecies);
        Ok(())
    }

    pub fn draw_fullscreen(&self, viewport: three_d::Viewport) {
        self.use_params();

        self.material.use_uniform_if_required(
            "viewport",
            Vector2::<i32>::new(viewport.width as i32, viewport.height as i32),
        );
        self.material
            .use_vertex_attribute("inPos", &self.vertecies_pos);

        self.material.draw_arrays(self.state, viewport, 3)
    }

    pub fn draw_camera(&self, camera: &dyn three_d::Viewer) {
        self.set_camera_uniforms(camera);

        self.material
            .draw_elements(self.state, camera.viewport(), &self.indecies);
    }

    #[inline]
    fn set_camera_uniforms(&self, camera: &dyn three_d::Viewer) {
        self.material.use_uniform("proj", camera.projection());
        self.material.use_uniform("camera", camera.view());
        self.material.use_uniform("world", self.transform);
        self.use_params();
        self.material
            .use_vertex_attribute("inPos", &self.vertecies_pos);
        if self.requires_in_tex {
            self.material
                .use_vertex_attribute("inTex", &self.vertecies_uv); //UVs
        }

        if let Some(colors) = self.vertecies_color.as_ref() {
            if self.requires_in_color {
                self.material.use_vertex_attribute("inColor", colors);
            }
        }
    }

    pub fn draw_instanced_camera<T>(
        &self,
        camera: &three_d::Camera,
        instances: impl IntoIterator<Item = T>,
        set_uniforms: impl Fn(&Program, &Mat4, T),
    ) {
        profile_function!();
        //TODO: Use actual instancing, may causes skin incompatibility
        let viewport = camera.viewport();
        for i in instances {
            self.set_camera_uniforms(camera);
            set_uniforms(&self.material, &self.transform, i);
            self.material
                .draw_elements(self.state, viewport, &self.indecies)
        }
    }

    pub fn set_data(
        &mut self,
        pos: &[Vector3<f32>],
        uv: &[Vector2<f32>],
        colors: &Option<Vec<Vector4<f32>>>,
    ) {
        self.set_data_indexed(pos, uv, &[] as &[u32], colors);
        self.update_indecies().expect("Bad mesh data");
    }

    pub fn set_data_mesh(&mut self, mesh: &three_d::CpuMesh) {
        profile_function!();
        self.aabb = mesh.compute_aabb();
        let colors: Option<Vec<Vec4>> = mesh
            .colors
            .as_ref()
            .map(|x| x.iter().copied().map(|x| x.into()).collect_vec());
        if let Some(indicies) = mesh.indices.to_u32() {
            self.set_data_indexed(
                &mesh.positions.to_f32(),
                mesh.uvs.as_ref().unwrap_or(&vec![]),
                &indicies,
                &colors,
            );
        } else {
            self.set_data(
                &mesh.positions.to_f32(),
                mesh.uvs.as_ref().unwrap_or(&vec![]),
                &colors,
            );
        }
    }

    pub fn set_data_indexed(
        &mut self,
        pos: &[Vector3<f32>],
        uv: &[Vector2<f32>],
        indecies: &[u32],
        colors: &Option<Vec<Vector4<f32>>>,
    ) {
        profile_function!();

        self.vertecies_pos.fill(pos);

        self.vertecies_pos.fill(pos);
        self.vertecies_uv.fill(uv);

        match (self.vertecies_color.as_mut(), colors) {
            (None, None) => {}
            (None, Some(x)) => {
                self.vertecies_color =
                    Some(VertexBuffer::new_with_data(&self.context, x.as_slice()))
            }
            (Some(_), None) => self.vertecies_color = None,
            (Some(buffer), Some(x)) => buffer.fill(x.as_slice()),
        }

        self.indecies.fill(indecies);
    }

    pub fn draw_lua_skin(
        &mut self,
        resolution: (u32, u32),
        vgfx: &RwLock<Vgfx>,
    ) -> Result<(), tealr::mlu::mlua::Error> {
        let [c0r0, c0r1, c1r0, c1r1, c2r0, c2r1] = {
            let vgfx = vgfx.write().expect("Lock error");
            let canvas = vgfx.canvas.lock().expect("Lock error");
            let transform = canvas.transform();
            //transform.scale(1.0, -1.0);

            transform.0
        };

        self.use_params();
        self.material.use_uniform(
            "proj",
            create_orthographic(
                0.0,
                resolution.0 as f32,
                resolution.1 as f32,
                0.0,
                0.0,
                100.0,
            ),
        );

        self.material.use_uniform(
            "world",
            Mat4::new(
                c0r0, c0r1, 0.0, 0.0, c1r0, c1r1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, c2r0, c2r1, 0.0,
                1.0,
            ),
        );
        self.material
            .use_vertex_attribute("inPos", &self.vertecies_pos);
        if self.material.requires_attribute("inTex") {
            self.material
                .use_vertex_attribute("inTex", &self.vertecies_uv); //UVs
        }
        self.material.draw_elements(
            self.state,
            three_d_asset::Viewport {
                x: 0,
                y: 0,
                width: resolution.0,
                height: resolution.1,
            },
            &self.indecies,
        );
        Ok(())
    }
}

impl Geometry for ShadedMesh {
    fn render_with_material(
        &self,
        _material: &dyn three_d::Material,
        camera: &dyn three_d::Viewer,
        _lights: &[&dyn three_d::Light],
    ) {
        self.draw_camera(camera);
    }

    fn aabb(&self) -> three_d::AxisAlignedBoundingBox {
        three_d::AxisAlignedBoundingBox::EMPTY
    }

    fn draw(&self, camera: &dyn three_d::Viewer, _program: &Program, _render_states: RenderStates) {
        self.draw_camera(camera);
    }

    fn vertex_shader_source(&self) -> String {
        todo!()
    }

    fn id(&self) -> three_d::GeometryId {
        todo!()
    }

    fn render_with_effect(
        &self,
        material: &dyn three_d::Effect,
        viewer: &dyn three_d::Viewer,
        lights: &[&dyn three_d::Light],
        color_texture: Option<three_d::ColorTexture>,
        depth_texture: Option<three_d::DepthTexture>,
    ) {
        todo!()
    }
}

impl Object for ShadedMesh {
    fn render(&self, camera: &dyn three_d::Viewer, _lights: &[&dyn three_d::Light]) {
        self.draw_camera(camera);
    }

    fn material_type(&self) -> three_d::MaterialType {
        three_d::MaterialType::Transparent
    }
}

#[derive(Debug, ToTypename, Clone, Copy)]
struct LuaVec2(f32, f32);

impl<'lua> FromLua<'lua> for LuaVec2 {
    fn from_lua(
        lua_value: tealr::mlu::mlua::Value<'lua>,
        _lua: &'lua Lua,
    ) -> tealr::mlu::mlua::Result<Self> {
        use tealr::mlu::mlua::Value;
        if let Value::Table(value) = lua_value {
            Ok(Self(value.get(1)?, value.get(2)?))
        } else {
            Err(tealr::mlu::mlua::Error::FromLuaConversionError {
                from: lua_value.type_name(),
                to: "LuaVec2",
                message: None,
            })
        }
    }
}

#[derive(Debug, ToTypename, Clone, Copy)]
struct LuaVert2(LuaVec2, LuaVec2);

impl<'lua> FromLua<'lua> for LuaVert2 {
    fn from_lua(
        lua_value: tealr::mlu::mlua::Value<'lua>,
        _lua: &'lua Lua,
    ) -> tealr::mlu::mlua::Result<Self> {
        use tealr::mlu::mlua::Value;
        if let Value::Table(value) = lua_value {
            Ok(Self(value.get(1)?, value.get(2)?))
        } else {
            Err(tealr::mlu::mlua::Error::FromLuaConversionError {
                from: lua_value.type_name(),
                to: "LuaVert2",
                message: None,
            })
        }
    }
}

fn create_orthographic(
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
    z_near: f32,
    z_far: f32,
) -> Mat4 {
    let c0r0 = 2f32 / (right - left);
    let c1r1 = 2f32 / (top - bottom);
    let c2r2 = -2f32 / (z_far - z_near);
    let c3r0 = -(right + left) / (right - left);
    let c3r1 = -(top + bottom) / (top - bottom);
    let c3r2 = -(z_far + z_near) / (z_far - z_near);
    let c3r3 = 1f32;
    Mat4::new(
        c0r0, 0.0, 0.0, 0.0, 0.0, c1r1, 0.0, 0.0, 0.0, 0.0, c2r2, 0.0, c3r0, c3r1, c3r2, c3r3,
    )
}

//TODO: Move methods to struct impl for reuse in other parts of the program
impl TealData for ShadedMesh {
    fn add_methods<'lua, T: tealr::mlu::TealDataMethods<'lua, Self>>(methods: &mut T) {
        methods.add_method_mut("Draw", |lua, this, _: ()| {
            let frame = lua
                .app_data_ref::<RefMut<game_data::GameData>>()
                .ok_or(mlua::Error::external("App data not set"))?
                .read()
                .expect("Lock error")
                .resolution;
            let vgfx = &lua
                .app_data_ref::<RefMut<Vgfx>>()
                .ok_or(mlua::Error::external("VGFX App data not set"))?;
            this.draw_lua_skin(frame, vgfx)
        });
        methods.add_method_mut("AddTexture", |_lua, this, params: (String, String)| {
            this.use_texture(params.0, params.1, (false, false), true)
                .map(|_| ())
                .map_err(tealr::mlu::mlua::Error::external)
        });
        methods.add_method_mut("AddSkinTexture", |_lua, this, params: (String, String)| {
            let mut path = GameConfig::get().game_folder.clone();
            let skin = &GameConfig::get().skin;
            path.push("skins");
            path.push(skin);
            path.push("textures");
            path.push(params.1);

            this.use_texture(params.0, path, (false, false), true)
                .map(|_| ())
                .map_err(tealr::mlu::mlua::Error::external)
        });
        methods.add_method_mut(
            "AddSharedTexture",
            |_lua, this, params: (String, String)| {
                this.use_texture(params.0, params.1, (false, false), true)
                    .map(|_| ())
                    .map_err(tealr::mlu::mlua::Error::external)
            },
        );

        methods.add_method_mut("SetParam", |_, this, params: (String, f32)| {
            this.set_param(params.0.as_str(), params.1);
            Ok(())
        });
        methods.add_method_mut("SetParamVec2", |_, this, params: (String, f32, f32)| {
            let data = vec2(params.1, params.2);
            this.set_param(params.0.as_str(), data);
            Ok(())
        });
        methods.add_method_mut(
            "SetParamVec3",
            |_, this, params: (String, f32, f32, f32)| {
                let data = vec3(params.1, params.2, params.3);
                this.set_param(params.0.as_str(), data);
                Ok(())
            },
        );
        methods.add_method_mut(
            "SetParamVec4",
            |_, this, params: (String, f32, f32, f32, f32)| {
                let data = vec4(params.1, params.2, params.3, params.4);
                this.set_param(params.0.as_str(), data);
                Ok(())
            },
        );

        methods.add_method_mut("SetData", |lua, this, (verts,): (Vec<LuaVert2>,)| {
            this.vertex_count = verts.len();
            let (pos, uv): (Vec<_>, Vec<_>) = verts
                .iter()
                .map(|vert| (vec3(vert.0 .0, vert.0 .1, 0.0), vec2(vert.1 .0, vert.1 .1)))
                .unzip();

            let context = &lua
                .app_data_ref::<Arc<three_d::Context>>()
                .ok_or(mlua::Error::external("three_d Context app data not set"))?;
            this.vertecies_pos = VertexBuffer::new_with_data(context, &pos);
            this.vertecies_uv = VertexBuffer::new_with_data(context, &uv);

            this.material
                .use_vertex_attribute("inPos", &this.vertecies_pos); //Vertex positions
            if this.material.requires_attribute("inTex") {
                this.material
                    .use_vertex_attribute("inTex", &this.vertecies_uv); //UVs
            }

            this.update_indecies()
                .map_err(tealr::mlu::mlua::Error::external)
        });
        methods.add_method_mut("SetBlendMode", |_, this, params: u8| {
            match params {
                0 => this.state.blend = Blend::TRANSPARENCY,
                1 => this.state.blend = Blend::ADD,
                2 => todo!(), //Multiply
                _ => {
                    return Err(tealr::mlu::mlua::Error::RuntimeError(format!(
                        "{params} is not a valid blend mode."
                    )))
                }
            }

            Ok(())
        });
        methods.add_method_mut("SetOpaque", |_, this, params: bool| {
            if params {
                this.state.blend = Blend::Disabled
            } else {
                this.state.blend = Blend::STANDARD_TRANSPARENCY
            }
            Ok(())
        });
        methods.add_method_mut("SetPrimitiveType", |_, this, params: u8| {
            match params {
                0 => this.draw_mode = DrawingMode::Triangles,
                1 => this.draw_mode = DrawingMode::Strip,
                2 => this.draw_mode = DrawingMode::Fan,
                _ => {
                    return Err(tealr::mlu::mlua::Error::RuntimeError(format!(
                        "{params} is not a valid drawing mode."
                    )))
                }
            }

            this.update_indecies()
                .map_err(tealr::mlu::mlua::Error::external)
        });
    }

    fn add_fields<'lua, F: tealr::mlu::TealDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_function_get("PRIM_TRILIST", |_, _| Ok(0));
        fields.add_field_function_get("PRIM_TRISTRIP", |_, _| Ok(1));
        fields.add_field_function_get("PRIM_TRIFAN", |_, _| Ok(2));

        fields.add_field_function_get::<_, u8, _>("PRIM_LINELIST", |_, _| lua_todo());
        fields.add_field_function_get::<_, u8, _>("PRIM_LINESTRIP", |_, _| lua_todo());
        fields.add_field_function_get::<_, u8, _>("PRIM_POINTLIST", |_, _| lua_todo());

        fields.add_field_function_get("BLEND_NORM", |_, _| Ok(0));
        fields.add_field_function_get("BLEND_ADD", |_, _| Ok(1));
        fields.add_field_function_get("BLEND_MULT", |_, _| Ok(2));
    }
}

fn lua_todo<T>() -> Result<T, tealr::mlu::mlua::Error> {
    Err(tealr::mlu::mlua::Error::RuntimeError(
        "Not implemented".into(),
    ))
}

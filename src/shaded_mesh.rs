use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::ensure;
use puffin::profile_function;
use tealr::{
    mlu::{
        mlua::{FromLua, Lua},
        TealData, UserData,
    },
    TypeName,
};
use three_d::{
    vec2, vec3, vec4, AxisAlignedBoundingBox, Blend, BufferDataType, CpuTexture, ElementBuffer,
    ElementBufferDataType, FrameInput, Geometry, Mat4, Object, Program, RenderStates, SquareMatrix,
    Texture2D, Vec2, Vec3, Vec4, VertexBuffer, Wrapping,
};

use crate::{config::GameConfig, vg_ui::Vgfx};

pub enum ShaderParam {
    Single(f32),
    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),
    Texture(Texture2D),
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

/// https://www.khronos.org/opengl/wiki/Primitive#Triangle_primitives for calculating indecies
enum DrawingMode {
    Triangles = 0,
    Fan = 1,
    Strip = 2,
}

#[derive(UserData, TypeName)]
pub struct ShadedMesh {
    params: HashMap<String, ShaderParam>,
    material: three_d::Program,
    state: RenderStates,
    vertex_count: usize,
    draw_mode: DrawingMode,
    indecies: ElementBuffer,
    vertecies_pos: VertexBuffer,
    vertecies_uv: VertexBuffer,
    vertecies_color: Option<VertexBuffer>,
    aabb: AxisAlignedBoundingBox,
    transform: Mat4,
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

        let vertex_shader_source: String = std::fs::read_to_string(&shader_path)?
            .lines()
            .filter(|x| !x.to_lowercase().contains("#version"))
            .collect::<Vec<_>>()
            .join("\n");
        let fragment_shader_source: String =
            std::fs::read_to_string(shader_path.with_extension("fs"))?
                .lines()
                .filter(|x| !x.to_lowercase().contains("#version"))
                .collect::<Vec<_>>()
                .join("\n");

        Ok(Self {
            params: HashMap::new(),
            material: Program::from_source(
                context,
                &vertex_shader_source,
                &fragment_shader_source,
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
            vertecies_pos: VertexBuffer::new(context),
            vertecies_uv: VertexBuffer::new(context),
            aabb: AxisAlignedBoundingBox::EMPTY,
            transform: Mat4::identity(),
            vertecies_color: None,
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
            DrawingMode::Strip => index_list.windows(3).flatten().copied().collect(),
            DrawingMode::Fan => index_list
                .windows(2)
                .skip(1)
                .flat_map(|x| [0, x[0], x[1]])
                .collect(),
        };

        self.indecies.fill(&indecies);

        Ok(())
    }

    pub fn set_param(&mut self, key: impl Into<String>, param: impl Into<ShaderParam>) {
        self.params.insert(key.into(), param.into());
    }

    pub fn use_texture(
        &mut self,
        context: &three_d::Context,
        name: impl Into<String>,
        path: impl AsRef<Path>,
        wrap_xy: (bool, bool),
    ) -> anyhow::Result<()> {
        profile_function!();
        let name = name.into();
        let mut texture: CpuTexture = three_d_asset::io::load_and_deserialize(path)?;

        log::info!("{}", &texture.name);
        dbg!(&texture.data);
        texture.data = match texture.data {
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

        match wrap_xy {
            (true, true) => {
                texture.wrap_s = Wrapping::Repeat;
                texture.wrap_t = Wrapping::Repeat;
            }
            (true, false) => {
                texture.wrap_s = Wrapping::Repeat;
                texture.wrap_t = Wrapping::ClampToEdge;
            }
            (false, true) => {
                texture.wrap_s = Wrapping::ClampToEdge;
                texture.wrap_t = Wrapping::Repeat;
            }
            (false, false) => {
                texture.wrap_s = Wrapping::ClampToEdge;
                texture.wrap_t = Wrapping::ClampToEdge;
            }
        }

        let texture = three_d::Texture2D::new(context, &texture);
        self.material.use_texture(&name, &texture);
        self.params.insert(name, ShaderParam::Texture(texture));
        Ok(())
    }

    fn use_params(&self) {
        for (name, param) in &self.params {
            if self.material.requires_uniform(name) {
                match param {
                    ShaderParam::Single(v) => self.material.use_uniform(name, v),
                    ShaderParam::Vec2(v) => self.material.use_uniform(name, v),
                    ShaderParam::Vec3(v) => self.material.use_uniform(name, v),
                    ShaderParam::Vec4(v) => self.material.use_uniform(name, v),
                    ShaderParam::Texture(v) => self.material.use_texture(name, v),
                }
            }
        }
    }

    fn draw(&self, frame: &FrameInput<()>) -> Result<(), tealr::mlu::mlua::Error> {
        self.use_params();
        self.material
            .use_vertex_attribute("inPos", &self.vertecies_pos);
        if self.material.requires_attribute("inTex") {
            self.material
                .use_vertex_attribute("inTex", &self.vertecies_uv); //UVs
        }

        if let Some(colors) = self.vertecies_color.as_ref() {
            if self.material.requires_attribute("inColor") {
                self.material.use_vertex_attribute("inColor", colors);
            }
        }

        self.material
            .draw_elements(self.state, frame.viewport, &self.indecies);
        Ok(())
    }

    pub fn draw_camera(&self, camera: &three_d::Camera) -> Result<(), tealr::mlu::mlua::Error> {
        self.material.use_uniform("proj", camera.projection());
        self.material.use_uniform("camera", camera.view());
        self.material.use_uniform("world", self.transform);
        self.use_params();
        self.material
            .use_vertex_attribute("inPos", &self.vertecies_pos);
        if self.material.requires_attribute("inTex") {
            self.material
                .use_vertex_attribute("inTex", &self.vertecies_uv); //UVs
        }

        if let Some(colors) = self.vertecies_color.as_ref() {
            if self.material.requires_attribute("inColor") {
                self.material.use_vertex_attribute("inColor", colors);
            }
        }

        self.material
            .draw_elements(self.state, camera.viewport(), &self.indecies);
        Ok(())
    }

    pub fn set_data<T: BufferDataType, U: BufferDataType, V: BufferDataType>(
        &mut self,
        context: &three_d::Context,
        pos: &[T],
        uv: &[U],
        colors: &Option<Vec<V>>,
    ) {
        self.set_data_indexed(context, pos, uv, &[] as &[u32], colors);
        self.update_indecies();
    }

    pub fn set_data_mesh(&mut self, context: &three_d::Context, mesh: &three_d::CpuMesh) {
        self.aabb = mesh.compute_aabb();

        if let Some(indicies) = mesh.indices.to_u32() {
            self.set_data_indexed(
                context,
                &mesh.positions.to_f32(),
                mesh.uvs.as_ref().unwrap_or(&vec![]),
                &indicies,
                &mesh.colors,
            );
        } else {
            self.set_data(
                context,
                &mesh.positions.to_f32(),
                mesh.uvs.as_ref().unwrap_or(&vec![]),
                &mesh.colors,
            );
        }
    }

    pub fn set_data_indexed<
        T: BufferDataType,
        U: BufferDataType,
        V: ElementBufferDataType,
        W: BufferDataType,
    >(
        &mut self,
        context: &three_d::Context,
        pos: &[T],
        uv: &[U],
        indecies: &[V],
        colors: &Option<Vec<W>>,
    ) {
        self.vertecies_pos = VertexBuffer::new_with_data(context, pos);
        self.vertecies_uv = VertexBuffer::new_with_data(context, uv);
        self.vertecies_color = colors
            .as_ref()
            .map(|x| VertexBuffer::new_with_data(context, x.as_slice()));
        self.indecies = ElementBuffer::new_with_data(context, indecies);
    }

    pub fn draw_lua_skin(
        &mut self,
        frame: &FrameInput<()>,
        vgfx: &Mutex<Vgfx>,
    ) -> Result<(), tealr::mlu::mlua::Error> {
        let _t = {
            let vgfx = vgfx.lock().unwrap();
            let canvas = vgfx.canvas.lock().unwrap();
            canvas.transform().to_mat3x4()
        };
        self.use_params();
        self.material.use_uniform(
            "proj",
            create_orthographic(
                0.0,
                frame.viewport.width as f32,
                frame.viewport.height as f32,
                0.0,
                0.0,
                100.0,
            ),
        );
        self.material.use_uniform("world", Mat4::from_scale(1.0));
        self.material
            .use_vertex_attribute("inPos", &self.vertecies_pos);
        if self.material.requires_attribute("inTex") {
            self.material
                .use_vertex_attribute("inTex", &self.vertecies_uv); //UVs
        }
        self.material
            .draw_elements(self.state, frame.viewport, &self.indecies);
        Ok(())
    }
}

impl Geometry for ShadedMesh {
    fn render_with_material(
        &self,
        _material: &dyn three_d::Material,
        camera: &three_d::Camera,
        _lights: &[&dyn three_d::Light],
    ) {
        self.draw_camera(camera);
    }

    fn render_with_post_material(
        &self,
        _material: &dyn three_d::PostMaterial,
        camera: &three_d::Camera,
        _lights: &[&dyn three_d::Light],
        _color_texture: Option<three_d::ColorTexture>,
        _depth_texture: Option<three_d::DepthTexture>,
    ) {
        self.draw_camera(camera);
    }

    fn aabb(&self) -> three_d::AxisAlignedBoundingBox {
        three_d::AxisAlignedBoundingBox::EMPTY
    }
}

impl Object for ShadedMesh {
    fn render(&self, camera: &three_d::Camera, _lights: &[&dyn three_d::Light]) {
        self.draw_camera(camera);
    }

    fn material_type(&self) -> three_d::MaterialType {
        three_d::MaterialType::Transparent
    }
}

#[derive(Debug, TypeName, Clone, Copy)]
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

#[derive(Debug, TypeName, Clone, Copy)]
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
            let frame = &lua.app_data_ref::<FrameInput<()>>().unwrap();
            let vgfx = &lua.app_data_ref::<Arc<Mutex<Vgfx>>>().unwrap();
            this.draw_lua_skin(frame, vgfx)
        });
        methods.add_method_mut("AddTexture", |lua, this, params: (String, String)| {
            let context = &lua.app_data_ref::<FrameInput<()>>().unwrap().context;
            this.use_texture(context, params.0, params.1, (false, false))
                .map_err(tealr::mlu::mlua::Error::external)
        });
        methods.add_method_mut("AddSkinTexture", |lua, this, params: (String, String)| {
            let context = &lua.app_data_ref::<FrameInput<()>>().unwrap().context;

            let mut path = std::env::current_dir().unwrap();
            let skin = &GameConfig::get().unwrap().skin;
            path.push("skins");
            path.push(skin);
            path.push("textures");
            path.push(params.1);

            this.use_texture(context, params.0, path, (false, false))
                .map_err(tealr::mlu::mlua::Error::external)
        });
        methods.add_method_mut("AddSharedTexture", |lua, this, params: (String, String)| {
            let context = &lua.app_data_ref::<FrameInput<()>>().unwrap().context;
            this.use_texture(context, params.0, params.1, (false, false))
                .map_err(tealr::mlu::mlua::Error::external)
        });

        methods.add_method_mut("SetParam", |_, this, params: (String, f32)| {
            this.set_param(params.0, params.1);
            Ok(())
        });
        methods.add_method_mut("SetParamVec2", |_, this, params: (String, f32, f32)| {
            let data = vec2(params.1, params.2);
            this.set_param(params.0, data);
            Ok(())
        });
        methods.add_method_mut(
            "SetParamVec3",
            |_, this, params: (String, f32, f32, f32)| {
                let data = vec3(params.1, params.2, params.3);
                this.set_param(params.0, data);
                Ok(())
            },
        );
        methods.add_method_mut(
            "SetParamVec4",
            |_, this, params: (String, f32, f32, f32, f32)| {
                let data = vec4(params.1, params.2, params.3, params.4);
                this.set_param(params.0, data);
                Ok(())
            },
        );

        methods.add_method_mut("SetData", |lua, this, (verts,): (Vec<LuaVert2>,)| {
            this.vertex_count = verts.len();
            let (pos, uv): (Vec<_>, Vec<_>) = verts
                .iter()
                .map(|vert| (vec2(vert.0 .0, vert.0 .1), vec2(vert.1 .0, vert.1 .1)))
                .unzip();

            let context = &lua.app_data_ref::<FrameInput<()>>().unwrap().context;
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
                0 => this.state.blend = Blend::STANDARD_TRANSPARENCY,
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
                1 => this.draw_mode = DrawingMode::Fan,
                2 => this.draw_mode = DrawingMode::Strip,
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
        fields.add_field_function_get("PRIM_TRIFAN", |_, _| Ok(1));
        fields.add_field_function_get("PRIM_TRISTRIP", |_, _| Ok(2));

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

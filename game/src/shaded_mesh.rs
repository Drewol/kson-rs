use std::{
    borrow::Cow,
    collections::HashMap,
    io::Read,
    path::Path,
    sync::{Arc, RwLock},
};

use anyhow::{anyhow, ensure};
use di::RefMut;
use encase::{
    internal::{BufferMut, WriteInto},
    ShaderSize, ShaderType,
};
use femtovg::renderer::WGPURenderer;
use futures::executor::block_on;
use image::{buffer, GenericImageView};
use itertools::Itertools;
use palette::{stimulus::IntoStimulus, IntoColor, Srgba};
use puffin::profile_function;
use tealr::{
    mlu::{
        mlua::{self, FromLua, Lua},
        TealData, UserData,
    },
    ToTypename,
};

use glam::{vec2, vec3, vec4, IVec2, IVec3, IVec4, Mat4, Vec2, Vec3, Vec4};
use wgpu::{
    naga::{self, front::glsl::Options},
    util::DeviceExt,
    BindGroupEntry, BindGroupLayoutEntry, BufferUsages, Device, Extent3d, FragmentState,
    ImageDataLayout, PrimitiveState, RenderPassColorAttachment, ShaderModuleDescriptor,
    ShaderStages, Texture, TextureDescriptor, TextureFormat,
};

use crate::{
    config::GameConfig,
    game::graphics::CpuMesh,
    game_data,
    help::{blend_add, transform_shader, RenderContext},
    vg_ui::Vgfx,
    FrameInput, Viewport,
};

pub enum ShaderParam {
    Int(i32),
    Single(f32),
    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),
    IVec2(IVec2),
    IVec3(IVec3),
    IVec4(IVec4),
    Texture(Texture),
}

impl From<IVec2> for ShaderParam {
    fn from(value: IVec2) -> Self {
        Self::IVec2(value)
    }
}

impl From<IVec3> for ShaderParam {
    fn from(value: IVec3) -> Self {
        Self::IVec3(value)
    }
}

impl From<IVec4> for ShaderParam {
    fn from(value: IVec4) -> Self {
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
impl From<wgpu::Texture> for ShaderParam {
    fn from(value: Texture) -> Self {
        Self::Texture(value)
    }
}

impl From<i32> for ShaderParam {
    fn from(value: i32) -> Self {
        Self::Int(value)
    }
}

// Reserved groups
// 1000 = transform
// 1001 = projection
// 1002 = camera
// 1003 = resolution

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
    material: wgpu::RenderPipeline,
    state: wgpu::BlendState,
    vertex_count: usize,
    draw_mode: DrawingMode,
    indecies: Vec<u32>,
    vertecies_pos: Vec<Vec3>,
    vertecies_uv: Vec<Vec2>,
    vertecies_color: Option<Vec<Vec4>>,
    vertex_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    transform: Mat4,
    context: RenderContext,
    /// Map uniform names to bind groups + buffer
    uniform_map: HashMap<String, (u32, wgpu::Buffer, wgpu::BindGroup)>,
}

#[derive(ShaderType)]
pub struct GpuVertex {
    pos: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
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
        context: &RenderContext,
        material: &str,
        path: impl AsRef<Path>,
    ) -> anyhow::Result<Self> {
        let mut shader_path = path.as_ref().to_path_buf();
        shader_path.push(material);
        shader_path.set_extension("wgsl");
        let fs_text = std::fs::read_to_string(shader_path)?;
        Self::new_with_shader(context, &fs_text, material)
    }

    pub fn new_with_shader(
        context: &RenderContext,
        shader_source: &str,
        name: &str,
    ) -> anyhow::Result<Self> {
        let RenderContext { device, .. } = context;
        let parser = naga::front::glsl::Frontend::default();

        profile_function!();

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some(name),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &[],
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 0,
            usage: BufferUsages::VERTEX,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 0,
            usage: BufferUsages::INDEX,
            mapped_at_creation: false,
        });

        let material = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: None,
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: GpuVertex::SHADER_SIZE.get(),
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4],
                }],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: None,
                compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
            }),
            multiview: None,
            cache: None,
        });

        let mut params = HashMap::new();

        Ok(Self {
            params,
            material,
            state: wgpu::BlendState::ALPHA_BLENDING,
            vertex_count: 0,
            draw_mode: DrawingMode::Triangles,
            indecies: vec![],
            vertecies_pos: vec![],
            vertecies_uv: vec![],
            transform: Mat4::IDENTITY,
            vertecies_color: None,
            context: context.clone(),
            uniform_bind_group,
            uniform_buffer,
            vertex_buffer,
            index_buffer,
            uniform_map: HashMap::new(),
        })
    }

    pub fn new_fullscreen(context: &RenderContext, fragment_shader: &str) -> anyhow::Result<Self> {
        let shader = format!(
            "{}\n{}",
            include_str!("static_assets/fullscreen.wgsl"),
            fragment_shader
        );

        let mut v = Self::new_with_shader(context, &shader, "Fullscreen")?;
        v.vertecies_pos = (0..3).map(|x| vec3(x as f32, 0.0, 0.0)).collect_vec();

        Ok(v)
    }

    pub fn with_transform(mut self, transform: Mat4) -> Self {
        self.transform = transform;
        self
    }

    pub fn write_uniform<T: ShaderType + WriteInto>(&mut self, v: &T) -> anyhow::Result<()> {
        let mut encase_buffer = encase::UniformBuffer::new(Vec::new());
        encase_buffer.write(v)?;
        self.context
            .queue
            .write_buffer(&self.uniform_buffer, 0, &encase_buffer.into_inner());
        Ok(())
    }

    pub fn set_blend(&mut self, blend: wgpu::BlendState) {
        self.state = blend;
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

        self.indecies = match self.draw_mode {
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

        Ok(())
    }

    pub fn set_param(&mut self, key: &str, param: impl Into<ShaderParam>) {
        if self.requires_uniform(key) {
            let key: String = key.into();
            self.params.insert(key, param.into());
        }
    }

    fn requires_uniform(&self, key: &str) -> bool {
        self.uniform_map.contains_key(key)
    }

    pub fn use_texture(
        &mut self,
        name: impl Into<String>,
        path: impl AsRef<Path>,
        wrap_xy: (bool, bool),
        flip_y: bool,
    ) -> anyhow::Result<Texture> {
        profile_function!();
        let name = name.into();
        let mut texture_data = image::open(path)?;

        texture_data = if flip_y {
            texture_data.flipv()
        } else {
            texture_data
        };

        let (wrap_h, wrap_v) = match wrap_xy {
            (true, true) => (wgpu::AddressMode::Repeat, wgpu::AddressMode::Repeat),
            (true, false) => (wgpu::AddressMode::Repeat, wgpu::AddressMode::ClampToBorder),
            (false, true) => (wgpu::AddressMode::ClampToBorder, wgpu::AddressMode::Repeat),
            (false, false) => (
                wgpu::AddressMode::ClampToBorder,
                wgpu::AddressMode::ClampToBorder,
            ),
        };

        let cpu_texture = self.context.device.create_texture_with_data(
            &self.context.queue,
            &TextureDescriptor {
                label: Some(&name),
                size: Extent3d {
                    width: texture_data.width(),
                    height: texture_data.height(),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            texture_data
                .as_rgba8()
                .ok_or(anyhow!("Failed to convert texture"))?
                .as_raw(),
        );

        Ok(cpu_texture)
    }

    pub fn use_uniform<T: ShaderType + WriteInto>(&self, n: impl AsRef<str>, v: T) {
        if let Some((_, buff, _)) = self.uniform_map.get(n.as_ref()) {
            let mut encased = encase::UniformBuffer::new(Vec::<u8>::new());
            encased.write(&v);

            self.context
                .queue
                .write_buffer(buff, 0, &encased.into_inner());
        }
    }

    pub fn use_texture_uniform(&self, name: &str, tex: &wgpu::Texture) {}

    #[inline]
    fn use_params(&self) {
        for (name, param) in &self.params {
            match param {
                ShaderParam::Single(v) => self.use_uniform(name, v),
                ShaderParam::Vec2(v) => self.use_uniform(name, v.to_array()),
                ShaderParam::Vec3(v) => self.use_uniform(name, v.to_array()),
                ShaderParam::Vec4(v) => self.use_uniform(name, v.to_array()),
                ShaderParam::IVec2(v) => self.use_uniform(name, v.to_array()),
                ShaderParam::IVec3(v) => self.use_uniform(name, v.to_array()),
                ShaderParam::IVec4(v) => self.use_uniform(name, v.to_array()),
                ShaderParam::Int(v) => self.use_uniform(name, v),
                ShaderParam::Texture(v) => self.use_texture_uniform(name, v),
            }
        }
    }

    #[allow(unused)]
    pub fn draw(&self) -> Result<(), tealr::mlu::mlua::Error> {
        self.use_params();
        let mut encoder = self.context.encoder(None);

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment {
                view: todo!(),
                resolve_target: todo!(),
                ops: todo!(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);

        // Set predefined bind groups
        // render_pass.set_bind_group(index, bind_group, offsets);

        // Set dynamic bind groups
        for (idx, _, g) in self.uniform_map.values() {
            render_pass.set_bind_group(*idx, g, &[]);
        }

        render_pass.draw_indexed(0..self.indecies.len() as u32, 0, 0..1);
        Ok(())
    }

    pub fn draw_fullscreen(&self, viewport: Viewport) {
        self.use_params();
        self.use_uniform("viewport", [viewport.width as i32, viewport.height as i32]);
        self.draw();
    }

    pub fn draw_camera(&self, camera: &Mat4) {
        self.set_camera_uniforms(camera);

        self.draw();
    }

    #[inline]
    fn set_camera_uniforms(&self, camera: &Mat4) {
        self.use_uniform("proj", camera.to_cols_array());
        self.use_uniform("camera", camera.to_cols_array());
        self.use_uniform("world", self.transform.to_cols_array());
        self.use_params();
    }

    pub fn draw_instanced_camera<T>(
        &self,
        camera: &Mat4,
        instances: impl IntoIterator<Item = T>,
        set_uniforms: impl Fn(&Self, Mat4, T),
        viewport: Viewport,
    ) {
        profile_function!();
        //TODO: Use actual instancing, may causes skin incompatibility

        for i in instances {
            self.set_camera_uniforms(camera);
            set_uniforms(&self, self.transform, i);
            self.draw();
        }
    }

    pub fn set_data(&mut self, pos: Vec<Vec3>, uv: Vec<Vec2>, colors: Option<Vec<Vec4>>) {
        self.set_data_indexed(pos, uv, vec![], colors);
        self.update_indecies().expect("Bad mesh data");
    }

    pub fn set_data_mesh(&mut self, mesh: &CpuMesh) {
        profile_function!();
        let colors: Option<Vec<Vec4>> = mesh
            .colors
            .as_ref()
            .map(|x| x.iter().copied().map(|x| x.into()).collect_vec());
        if let Some(indicies) = mesh.indices.as_ref() {
            self.set_data_indexed(
                mesh.positions.clone(),
                mesh.uvs.clone().unwrap_or(vec![]),
                indicies.clone(),
                colors.clone(),
            );
        } else {
            self.set_data(
                mesh.positions.clone(),
                mesh.uvs.clone().unwrap_or(vec![]),
                colors.clone(),
            );
        }
    }

    pub fn set_data_indexed(
        &mut self,
        pos: Vec<Vec3>,
        uv: Vec<Vec2>,
        indecies: Vec<u32>,
        colors: Option<Vec<Vec4>>,
    ) {
        profile_function!();

        self.vertecies_pos = pos;
        self.vertecies_uv = uv;
        self.vertecies_color = colors;
        self.indecies = if indecies.is_empty() {
            (0..self.vertecies_pos.len() as u32).collect_vec()
        } else {
            indecies
        };
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
        self.use_uniform(
            "proj",
            create_orthographic(
                0.0,
                resolution.0 as f32,
                resolution.1 as f32,
                0.0,
                0.0,
                100.0,
            )
            .to_cols_array(),
        );

        self.use_uniform(
            "world",
            Mat4::from_cols(
                vec4(c0r0, c0r1, 0.0, 0.0),
                vec4(c1r0, c1r1, 0.0, 0.0),
                vec4(0.0, 0.0, 0.0, 0.0),
                vec4(c2r0, c2r1, 0.0, 1.0),
            )
            .to_cols_array(),
        );

        self.draw();
        Ok(())
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
    Mat4::from_cols(
        vec4(c0r0, 0.0, 0.0, 0.0),
        vec4(0.0, c1r1, 0.0, 0.0),
        vec4(0.0, 0.0, c2r2, 0.0),
        vec4(c3r0, c3r1, c3r2, c3r3),
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

            this.vertecies_pos = pos;
            this.vertecies_uv = uv;

            this.update_indecies()
                .map_err(tealr::mlu::mlua::Error::external)
        });
        methods.add_method_mut("SetBlendMode", |_, this, params: u8| {
            match params {
                0 => this.state = wgpu::BlendState::ALPHA_BLENDING,
                1 => this.state = blend_add(),
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
                this.state = wgpu::BlendState::REPLACE;
            } else {
                this.state = wgpu::BlendState::ALPHA_BLENDING;
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

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    hash::{Hash, Hasher},
    path::Path,
    sync::RwLock,
};

use anyhow::{anyhow, ensure, Context};
use bytemuck::{bytes_of, Pod, Zeroable};
use di::RefMut;
use futures::executor::block_on;
use itertools::Itertools;
use log::info;
use mlua::{self, ExternalResult, FromLua, Lua, UserData, UserDataFields, UserDataMethods};
use puffin::{profile_function, profile_scope};

use glam::{vec2, vec3, vec4, IVec2, IVec3, IVec4, Mat4, Vec2, Vec3, Vec4};
use three_d_asset::Camera;
use wgpu::{
    naga, util::DeviceExt, BufferUsages, Extent3d, FragmentState, PrimitiveState,
    SamplerDescriptor, ShaderModuleDescriptor, ShaderStages, TextureDescriptor,
    TextureViewDescriptor,
};

use crate::{
    config::GameConfig,
    game::{camera::WgpuCameraWrapper, graphics::CpuMesh},
    game_data,
    help::{blend_add, RenderContext},
    lua_service::LuaExt,
    vg_ui::{Vgfx, VgfxSyncExt},
    Viewport,
};

#[derive(PartialEq)]
pub enum ShaderParam {
    Int(i32),
    Single(f32),
    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),
    IVec2(IVec2),
    IVec3(IVec3),
    IVec4(IVec4),
}

macro_rules! shader_param_from {
    ($($t:ty => $v:ident),*) => {
        $(impl From<$t> for ShaderParam {
            fn from(value: $t) -> Self {
                Self::$v(value)
            }
        })*
    };
}

shader_param_from!(
    i32 => Int,
    f32 => Single,
    Vec2 => Vec2,
    Vec3 => Vec3,
    Vec4 => Vec4,
    IVec2 => IVec2,
    IVec3 => IVec3,
    IVec4 => IVec4
);

/// https://www.khronos.org/opengl/wiki/Primitive#Triangle_primitives for calculating indecies
enum DrawingMode {
    Triangles = 0,
    Fan = 1,
    Strip = 2,
}

enum UniformValue {
    Buffer(wgpu::Buffer),
    Texture(wgpu::TextureView),
    Sampler(wgpu::Sampler),
}

impl UniformValue {
    fn as_binding(&self) -> wgpu::BindingResource<'_> {
        match self {
            UniformValue::Buffer(buffer) => buffer.as_entire_binding(),
            UniformValue::Texture(view) => wgpu::BindingResource::TextureView(view),
            UniformValue::Sampler(sampler) => wgpu::BindingResource::Sampler(sampler),
        }
    }

    fn binding_type(&self) -> wgpu::BindingType {
        match self {
            UniformValue::Buffer(_) => wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            UniformValue::Texture(_) => wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            UniformValue::Sampler(_) => {
                wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)
            }
        }
    }
}

struct UniformEntry {
    name: String,
    binding: u32,
    value: UniformValue,
}

struct UniformGroup {
    layout: wgpu::BindGroupLayout,
    entries: Vec<UniformEntry>,
    bind_group: wgpu::BindGroup,
}

impl UniformGroup {
    fn rebuild(&mut self, device: &wgpu::Device) {
        self.bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &self
                .entries
                .iter()
                .map(|x| wgpu::BindGroupEntry {
                    binding: x.binding,
                    resource: x.value.as_binding(),
                })
                .collect_vec(),
        });
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct GpuVertex {
    pos: [f32; 3],
    uv: [f32; 2],
    color: [f32; 4],
}

impl GpuVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4];
}

/// Remaps OpenGL clip space depth (-1..1) to wgpu clip space depth (0..1)
const GL_TO_WGPU_DEPTH: Mat4 = Mat4::from_cols_array(&[
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 0.5, 0.0, //
    0.0, 0.0, 0.5, 1.0, //
]);

//TODO: Cloneable with Arc for gpu resources for better shader reuse
pub struct ShadedMesh {
    params: HashMap<String, ShaderParam>,
    pipelines: RefCell<HashMap<wgpu::BlendState, wgpu::RenderPipeline>>,
    state: wgpu::BlendState,
    vertex_count: usize,
    draw_mode: DrawingMode,
    indecies: Vec<u32>,
    vertecies_pos: Vec<Vec3>,
    vertecies_uv: Vec<Vec2>,
    vertecies_color: Option<Vec<Vec4>>,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    transform: Mat4,
    context: RenderContext,
    /// Map uniform names to their bind group number
    uniform_map: HashMap<String, u32>,
    groups: BTreeMap<u32, UniformGroup>,
    name: String,
    layout: wgpu::PipelineLayout,
    vertex_shader: wgpu::ShaderModule,
    fragment_shader: wgpu::ShaderModule,
    uniform_hashes: RefCell<HashMap<String, u64>>,
    framebuffer_tex: Option<wgpu::Texture>,
}

impl ShadedMesh {
    const TEXFRAMEBUFFER_NAME: &str = "texFrameBuffer";

    pub fn new(
        context: &RenderContext,
        material: &str,
        path: impl AsRef<Path>,
    ) -> anyhow::Result<Self> {
        let mut shader_path = path.as_ref().to_path_buf();
        shader_path.push(material);

        // Prefer WGSL, fall back to legacy GLSL (.vs + .fs) shaders
        shader_path.set_extension("wgsl");
        if shader_path.exists() {
            let source = std::fs::read_to_string(&shader_path)
                .with_context(|| format!("Failed loading shader: {shader_path:?}"))?;
            return Self::new_with_shader(context, &source, material)
                .with_context(|| format!("Failed loading shader: {shader_path:?}"));
        }

        shader_path.set_extension("vs");
        let vs = std::fs::read_to_string(&shader_path)
            .with_context(|| format!("Failed loading shader: {shader_path:?}"))?;
        shader_path.set_extension("fs");
        let fs = std::fs::read_to_string(&shader_path)
            .with_context(|| format!("Failed loading shader: {shader_path:?}"))?;

        Self::new_with_glsl(context, &vs, &fs, material)
            .with_context(|| format!("Failed loading shader: {shader_path:?}"))
    }

    /// Creates a mesh from a WGSL shader containing both the vertex and fragment entry points
    pub fn new_with_shader(
        context: &RenderContext,
        shader_source: &str,
        name: &str,
    ) -> anyhow::Result<Self> {
        let module = naga::front::wgsl::Frontend::new().parse(shader_source)?;
        Self::from_modules(context, module.clone(), module, name)
    }

    /// Creates a mesh from a legacy GLSL shader pair, see [`crate::glsl_compat`]
    pub fn new_with_glsl(
        context: &RenderContext,
        vertex_source: &str,
        fragment_source: &str,
        name: &str,
    ) -> anyhow::Result<Self> {
        let (vs, fs) = crate::glsl_compat::parse(vertex_source, fragment_source)?;
        Self::from_modules(context, vs, fs, name)
    }

    fn from_modules(
        context: &RenderContext,
        vertex: naga::Module,
        fragment: naga::Module,
        name: &str,
    ) -> anyhow::Result<Self> {
        profile_function!();
        let device = &context.device;

        let mut uniform_map = HashMap::new();
        // group -> binding -> (module, global)
        let mut group_globals: BTreeMap<u32, BTreeMap<u32, (&naga::Module, &naga::GlobalVariable)>> =
            BTreeMap::new();

        // Add uniforms through reflection, both stages must agree on bindings
        for module in [&vertex, &fragment] {
            for (_, global) in module.global_variables.iter() {
                if let Some(binding) = global.binding.as_ref() {
                    group_globals
                        .entry(binding.group)
                        .or_default()
                        .entry(binding.binding)
                        .or_insert((module, global));
                }
            }
        }

        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

        let mut groups = BTreeMap::new();
        for (group, globals) in group_globals.iter() {
            let mut entries = vec![];

            for (module, global) in globals.values() {
                let (Some(name), Some(binding)) = (global.name.as_ref(), global.binding.as_ref())
                else {
                    continue;
                };

                let ty = &module.types[global.ty];

                let value = match ty.inner {
                    naga::TypeInner::Image { .. } => UniformValue::Texture(
                        device
                            .create_texture(&wgpu::TextureDescriptor {
                                label: Some(name.as_str()),
                                size: Extent3d {
                                    width: 1,
                                    height: 1,
                                    depth_or_array_layers: 1,
                                },
                                mip_level_count: 1,
                                sample_count: 1,
                                dimension: wgpu::TextureDimension::D2,
                                format: wgpu::TextureFormat::Rgba8Unorm,
                                usage: wgpu::TextureUsages::COPY_DST
                                    | wgpu::TextureUsages::TEXTURE_BINDING,
                                view_formats: &[],
                            })
                            .create_view(&TextureViewDescriptor::default()),
                    ),
                    naga::TypeInner::Sampler { .. } => UniformValue::Sampler(
                        device.create_sampler(&wgpu::SamplerDescriptor::default()),
                    ),
                    _ => UniformValue::Buffer(device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(name.as_str()),
                        mapped_at_creation: false,
                        size: ty.inner.size(module.to_ctx()).max(4) as _,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    })),
                };

                uniform_map.insert(name.clone(), *group);
                entries.push(UniformEntry {
                    name: name.clone(),
                    binding: binding.binding,
                    value,
                });
            }

            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(name),
                entries: &entries
                    .iter()
                    .map(|x| wgpu::BindGroupLayoutEntry {
                        binding: x.binding,
                        visibility: ShaderStages::VERTEX_FRAGMENT,
                        ty: x.value.binding_type(),
                        count: None,
                    })
                    .collect_vec(),
            });

            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(name),
                layout: &layout,
                entries: &entries
                    .iter()
                    .map(|x| wgpu::BindGroupEntry {
                        binding: x.binding,
                        resource: x.value.as_binding(),
                    })
                    .collect_vec(),
            });

            groups.insert(
                *group,
                UniformGroup {
                    layout,
                    entries,
                    bind_group,
                },
            );
        }

        // Pipeline layouts are indexed by group number, fill any gaps with empty layouts
        let empty_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Empty"),
            entries: &[],
        });
        let max_group = groups.keys().max().map(|x| x + 1).unwrap_or(0);
        let bind_group_layouts = (0..max_group)
            .map(|g| Some(groups.get(&g).map(|x| &x.layout).unwrap_or(&empty_layout)))
            .collect_vec();

        drop(group_globals);
        let vertex_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some(name),
            source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(vertex)),
        });
        let fragment_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some(name),
            source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(fragment)),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(name),
            bind_group_layouts: &bind_group_layouts,
            immediate_size: 0,
        });

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(name),
            size: 0,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(name),
            size: 0,
            usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut res = Self {
            params: HashMap::new(),
            pipelines: RefCell::new(HashMap::new()),
            state: wgpu::BlendState::ALPHA_BLENDING,
            vertex_count: 0,
            draw_mode: DrawingMode::Triangles,
            indecies: vec![],
            vertecies_pos: vec![],
            vertecies_uv: vec![],
            transform: Mat4::IDENTITY,
            vertecies_color: None,
            context: context.clone(),
            vertex_buffer,
            index_buffer,
            uniform_map,
            groups,
            name: name.to_string(),
            layout: pipeline_layout,
            vertex_shader,
            fragment_shader,
            uniform_hashes: RefCell::new(HashMap::default()),
            framebuffer_tex: None,
        };

        // Create the default pipeline now so shader errors are reported at load time
        res.pipeline(res.state);

        if let Some(err) = block_on(error_scope.pop()) {
            return Err(anyhow!("{err}"));
        }

        res.set_param("color", vec4(1.0, 1.0, 1.0, 1.0));

        Ok(res)
    }

    pub fn new_fullscreen(context: &RenderContext, fragment_shader: &str) -> anyhow::Result<Self> {
        let shader = format!(
            "{}\n{}",
            include_str!("static_assets/fullscreen.wgsl"),
            fragment_shader
        );

        let mut v = Self::new_with_shader(context, &shader, "Fullscreen")?;
        v.vertex_count = 3;
        v.vertecies_pos = (0..3).map(|x| vec3(x as f32, 0.0, 0.0)).collect_vec();
        v.update_indecies()?;

        Ok(v)
    }

    /// Fullscreen mesh using a legacy GLSL fragment shader with the old fullscreen vertex shader
    pub fn new_fullscreen_glsl(
        context: &RenderContext,
        fragment_shader: &str,
    ) -> anyhow::Result<Self> {
        let mut v = Self::new_with_glsl(
            context,
            include_str!("static_assets/fullscreen.vs"),
            fragment_shader,
            "Fullscreen",
        )?;
        v.vertex_count = 3;
        v.vertecies_pos = (0..3).map(|x| vec3(x as f32, 0.0, 0.0)).collect_vec();
        v.update_indecies()?;

        Ok(v)
    }

    pub fn new_fullscreen_tex(context: &RenderContext) -> anyhow::Result<Self> {
        Self::new_fullscreen(context, include_str!("static_assets/fullscreen_tex.wgsl"))
    }

    pub fn with_transform(mut self, transform: Mat4) -> Self {
        self.transform = transform;
        self
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
            self.vertex_count.is_multiple_of(vertex_multiple),
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
        self.update_vertex_buffers();
        Ok(())
    }

    fn update_vertex_buffers(&mut self) {
        profile_function!(&self.name);
        let device = &self.context.device;
        let queue = &self.context.queue;

        let colors = self.vertecies_color.as_deref().unwrap_or_default();
        let vertecies = self
            .vertecies_pos
            .iter()
            .enumerate()
            .map(|(i, pos)| GpuVertex {
                pos: pos.to_array(),
                uv: self.vertecies_uv.get(i).unwrap_or(&Vec2::ZERO).to_array(),
                color: colors.get(i).unwrap_or(&Vec4::ONE).to_array(),
            })
            .collect_vec();

        let vertex_data: &[u8] = bytemuck::cast_slice(&vertecies);
        let index_data: &[u8] = bytemuck::cast_slice(&self.indecies);

        let grow = |buffer: &mut wgpu::Buffer, size: u64, usage: BufferUsages, label: &str| {
            if buffer.size() < size {
                // Grow with some headroom to avoid reallocating every frame for dynamic meshes
                let size = (size + size / 2).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT);
                info!("Reallocating {label} buffer of {} to {size}b", self.name);
                *buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size,
                    usage: usage | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
        };

        grow(
            &mut self.index_buffer,
            index_data.len() as u64,
            BufferUsages::INDEX,
            "Index",
        );
        grow(
            &mut self.vertex_buffer,
            vertex_data.len() as u64,
            BufferUsages::VERTEX,
            "Vertex",
        );

        // Buffer writes must be a multiple of 4 bytes, vertices always are, indices are u32
        if !index_data.is_empty() {
            queue.write_buffer(&self.index_buffer, 0, index_data);
        }
        if !vertex_data.is_empty() {
            queue.write_buffer(&self.vertex_buffer, 0, vertex_data);
        }
    }

    pub fn set_param(&mut self, key: &str, param: impl Into<ShaderParam>) {
        if self.requires_uniform(key) {
            self.params.insert(key.into(), param.into());
        }
    }

    pub fn requires_uniform(&self, key: &str) -> bool {
        self.uniform_map.contains_key(key)
    }

    /// Binds `tex` to the texture uniform `name`, if the bind group has a sampler named `{name}_s`
    /// (or only a single sampler), that sampler is replaced to use the given wrapping.
    pub fn use_texture_existing(
        &mut self,
        name: impl AsRef<str>,
        tex: &wgpu::Texture,
        wrap_xy: (bool, bool),
    ) -> anyhow::Result<()> {
        let name = name.as_ref();
        let group = self
            .uniform_map
            .get(name)
            .and_then(|g| self.groups.get_mut(g))
            .ok_or_else(|| anyhow!("No texture {:?} in shader {:?}", name, self.name))?;

        let address_mode = |wrap| {
            if wrap {
                wgpu::AddressMode::Repeat
            } else {
                wgpu::AddressMode::ClampToEdge
            }
        };

        let sampler_name = format!("{name}_s");
        let sampler_count = group
            .entries
            .iter()
            .filter(|x| matches!(x.value, UniformValue::Sampler(_)))
            .count();

        let mut found = false;
        for entry in group.entries.iter_mut() {
            match &entry.value {
                UniformValue::Texture(_) if entry.name == name => {
                    entry.value =
                        UniformValue::Texture(tex.create_view(&TextureViewDescriptor::default()));
                    found = true;
                }
                UniformValue::Sampler(_) if entry.name == sampler_name || sampler_count == 1 => {
                    entry.value =
                        UniformValue::Sampler(self.context.device.create_sampler(
                            &SamplerDescriptor {
                                label: Some(&sampler_name),
                                address_mode_u: address_mode(wrap_xy.0),
                                address_mode_v: address_mode(wrap_xy.1),
                                address_mode_w: wgpu::AddressMode::Repeat,
                                mag_filter: wgpu::FilterMode::Linear,
                                min_filter: wgpu::FilterMode::Linear,
                                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                                ..Default::default()
                            },
                        ));
                }
                _ => {}
            }
        }

        ensure!(
            found,
            "Uniform {:?} in shader {:?} is not a texture",
            name,
            self.name
        );

        group.rebuild(&self.context.device);
        Ok(())
    }

    pub fn use_texture(
        &mut self,
        name: impl Into<String>,
        path: impl AsRef<Path>,
        wrap_xy: (bool, bool),
        flip_y: bool,
    ) -> anyhow::Result<wgpu::Texture> {
        profile_function!();
        let name = name.into();
        let path = path.as_ref();
        let mut texture_data =
            image::open(path).with_context(|| format!("Failed to load texture {path:?}"))?;

        // three-d (used by the OpenGL renderer) flipped textures again on upload,
        // invert the flag here to keep the same texture orientation as skins expect
        if !flip_y {
            texture_data = texture_data.flipv();
        }

        let texture = self.context.device.create_texture_with_data(
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
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            texture_data.to_rgba8().as_raw(),
        );
        self.use_texture_existing(name, &texture, wrap_xy)?;
        Ok(texture)
    }

    /// Copies `source` into the `texFrameBuffer` texture if the shader uses it.
    /// Must be recorded before the render pass that draws this mesh.
    pub fn set_tex_from_framebuffer(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Texture,
    ) -> anyhow::Result<()> {
        if !self.requires_uniform(Self::TEXFRAMEBUFFER_NAME) {
            return Ok(()); // Doesnt use framebuffer texture
        }

        let recreate = self
            .framebuffer_tex
            .as_ref()
            .is_none_or(|t| t.size() != source.size() || t.format() != source.format());

        if recreate {
            let tex = self.context.device.create_texture(&TextureDescriptor {
                label: Some(Self::TEXFRAMEBUFFER_NAME),
                size: source.size(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: source.format(),
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.use_texture_existing(Self::TEXFRAMEBUFFER_NAME, &tex, (false, false))?;
            self.framebuffer_tex = Some(tex);
        }

        let Some(tex) = self.framebuffer_tex.as_ref() else {
            return Ok(());
        };

        encoder.copy_texture_to_texture(
            source.as_image_copy(),
            tex.as_image_copy(),
            source.size(),
        );

        Ok(())
    }

    pub fn use_uniform<T: bytemuck::NoUninit>(&self, n: impl AsRef<str>, v: &T) {
        let data = bytes_of(v);
        let key = n.as_ref();

        let Some(group) = self.uniform_map.get(key).and_then(|g| self.groups.get(g)) else {
            return;
        };

        let Some(UniformValue::Buffer(buff)) = group
            .entries
            .iter()
            .find(|x| x.name == key)
            .map(|x| &x.value)
        else {
            return;
        };

        let mut hasher = std::hash::DefaultHasher::new();
        data.hash(&mut hasher);
        let new_hash = hasher.finish();

        let mut uniform_hashes = self.uniform_hashes.borrow_mut();
        if uniform_hashes.get(key) == Some(&new_hash) {
            return;
        }

        if data.len() as u64 > buff.size() {
            log::warn!(
                "Uniform {key:?} in {:?} is {}b but {}b was provided",
                self.name,
                buff.size(),
                data.len()
            );
            return;
        }

        profile_scope!("Write uniform");
        uniform_hashes.insert(key.to_owned(), new_hash);
        self.context.queue.write_buffer(buff, 0, data);
    }

    fn use_params(&self) {
        for (name, param) in &self.params {
            match param {
                ShaderParam::Single(v) => self.use_uniform(name, v),
                ShaderParam::Vec2(v) => self.use_uniform(name, v),
                ShaderParam::Vec3(v) => self.use_uniform(name, v),
                ShaderParam::Vec4(v) => self.use_uniform(name, v),
                ShaderParam::IVec2(v) => self.use_uniform(name, v),
                ShaderParam::IVec3(v) => self.use_uniform(name, v),
                ShaderParam::IVec4(v) => self.use_uniform(name, v),
                ShaderParam::Int(v) => self.use_uniform(name, v),
            }
        }
    }

    fn pipeline(&self, blend: wgpu::BlendState) -> wgpu::RenderPipeline {
        self.pipelines
            .borrow_mut()
            .entry(blend)
            .or_insert_with(|| {
                profile_scope!("Create pipeline");
                self.context
                    .device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some(&self.name),
                        layout: Some(&self.layout),
                        vertex: wgpu::VertexState {
                            module: &self.vertex_shader,
                            entry_point: None,
                            compilation_options: Default::default(),
                            buffers: &[Some(wgpu::VertexBufferLayout {
                                array_stride: std::mem::size_of::<GpuVertex>() as _,
                                step_mode: wgpu::VertexStepMode::Vertex,
                                attributes: &GpuVertex::ATTRIBUTES,
                            })],
                        },
                        primitive: PrimitiveState::default(),
                        depth_stencil: None,
                        multisample: Default::default(),
                        fragment: Some(FragmentState {
                            module: &self.fragment_shader,
                            entry_point: None,
                            compilation_options: Default::default(),
                            targets: &[Some(wgpu::ColorTargetState {
                                format: self.context.surface_config.format,
                                blend: Some(blend),
                                write_mask: wgpu::ColorWrites::ALL,
                            })],
                        }),
                        multiview_mask: None,
                        cache: None,
                    })
            })
            .clone()
    }

    pub fn draw(&self, render_pass: &mut wgpu::RenderPass) -> anyhow::Result<()> {
        profile_function!(&self.name);
        if self.vertecies_pos.is_empty() || self.indecies.is_empty() {
            return Ok(()); // Nothing to draw
        }

        self.use_params();

        render_pass.set_pipeline(&self.pipeline(self.state));
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);

        for (idx, g) in self.groups.iter() {
            render_pass.set_bind_group(*idx, &g.bind_group, &[]);
        }

        render_pass.draw_indexed(0..self.indecies.len() as u32, 0, 0..1);

        Ok(())
    }

    pub fn draw_fullscreen(
        &self,
        viewport: Viewport,
        render_pass: &mut wgpu::RenderPass,
    ) -> anyhow::Result<()> {
        self.use_uniform("viewport", &[viewport.width, viewport.height]);
        self.draw(render_pass)
    }

    pub fn draw_camera(
        &self,
        camera: &Camera,
        render_pass: &mut wgpu::RenderPass,
    ) -> anyhow::Result<()> {
        profile_function!(&self.name);
        self.set_camera_uniforms(camera);
        self.draw(render_pass)
    }

    #[inline]
    fn set_camera_uniforms(&self, camera: &Camera) {
        let proj = GL_TO_WGPU_DEPTH * Mat4::from_cols_array(&camera.wgpu_proj());
        self.use_uniform("proj", &proj.to_cols_array());
        self.use_uniform("camera", &camera.wgpu_view());
        self.use_uniform("world", &self.transform.to_cols_array());
    }

    pub fn set_data(&mut self, pos: Vec<Vec3>, uv: Vec<Vec2>, colors: Option<Vec<Vec4>>) {
        self.set_data_indexed(pos, uv, vec![], colors);
    }

    pub fn set_data_mesh(&mut self, mesh: &CpuMesh) {
        profile_function!();
        let uvs = mesh.uvs.clone().unwrap_or_default();
        if let Some(indicies) = mesh.indices.as_ref() {
            self.set_data_indexed(
                mesh.positions.clone(),
                uvs,
                indicies.clone(),
                mesh.colors.clone(),
            );
        } else {
            self.set_data(mesh.positions.clone(), uvs, mesh.colors.clone());
        }
    }

    pub fn set_data_indexed(
        &mut self,
        pos: Vec<Vec3>,
        uv: Vec<Vec2>,
        indecies: Vec<u32>,
        colors: Option<Vec<Vec4>>,
    ) {
        profile_function!(&self.name);

        self.vertex_count = pos.len();
        self.vertecies_pos = pos;
        self.vertecies_uv = uv;
        self.vertecies_color = colors;
        self.indecies = if indecies.is_empty() {
            (0..self.vertecies_pos.len() as u32).collect_vec()
        } else {
            indecies
        };

        self.update_vertex_buffers();
    }

    pub fn draw_lua_skin(
        &mut self,
        resolution: (u32, u32),
        vgfx: &RwLock<Vgfx>,
        render_pass: &mut wgpu::RenderPass,
    ) -> anyhow::Result<()> {
        let [c0r0, c0r1, c1r0, c1r1, c2r0, c2r1] = {
            let vgfx = vgfx.read().expect("Lock error");
            let canvas = vgfx.canvas.lock().expect("Lock error");
            canvas.transform().0
        };

        let proj = GL_TO_WGPU_DEPTH
            * create_orthographic(
                0.0,
                resolution.0 as f32,
                resolution.1 as f32,
                0.0,
                0.0,
                100.0,
            );
        self.use_uniform("proj", &proj.to_cols_array());

        self.use_uniform(
            "world",
            &Mat4::from_cols(
                vec4(c0r0, c0r1, 0.0, 0.0),
                vec4(c1r0, c1r1, 0.0, 0.0),
                vec4(0.0, 0.0, 0.0, 0.0),
                vec4(c2r0, c2r1, 0.0, 1.0),
            )
            .to_cols_array(),
        );

        self.draw(render_pass)
    }
}

#[derive(Debug, Clone, Copy)]
struct LuaVec2(f32, f32);

impl FromLua for LuaVec2 {
    fn from_lua(lua_value: mlua::Value, _lua: &Lua) -> mlua::Result<Self> {
        use mlua::Value;
        if let Value::Table(value) = lua_value {
            Ok(Self(value.get(1)?, value.get(2)?))
        } else {
            Err(mlua::Error::FromLuaConversionError {
                from: lua_value.type_name(),
                to: "LuaVec2".to_owned(),
                message: None,
            })
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct LuaVert2(LuaVec2, LuaVec2);

impl FromLua for LuaVert2 {
    fn from_lua(lua_value: mlua::Value, _lua: &Lua) -> mlua::Result<Self> {
        use mlua::Value;
        if let Value::Table(value) = lua_value {
            Ok(Self(value.get(1)?, value.get(2)?))
        } else {
            Err(mlua::Error::FromLuaConversionError {
                from: lua_value.type_name(),
                to: "LuaVert2".to_owned(),
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
impl UserData for ShadedMesh {
    fn add_methods<T: UserDataMethods<Self>>(methods: &mut T) {
        methods.add_method_mut("Draw", |lua, this, _: ()| {
            let frame = lua
                .app_data_ref::<RefMut<game_data::GameData>>()
                .ok_or(mlua::Error::external("App data not set"))?
                .read()
                .expect("Lock error")
                .resolution;
            let vgfx = lua
                .app_data_ref::<RefMut<Vgfx>>()
                .ok_or(mlua::Error::external("VGFX App data not set"))?
                .clone();

            if let Some(mut rp) = lua.render_pass() {
                this.draw_lua_skin(frame, &vgfx, &mut rp).into_lua_err()
            } else {
                let mut encoder = lua.wgpu_encoder(Some("Lua ShadedMesh"));
                let ctx = lua.render_context();
                let surface = lua.wgpu_surface();
                this.set_tex_from_framebuffer(&mut encoder, &surface.texture)
                    .into_lua_err()?;
                let mut rp = ctx.default_render_pass(None, &mut encoder, surface.as_ref());

                this.draw_lua_skin(frame, &vgfx, &mut rp).into_lua_err()?;
                drop(rp);
                vgfx.extend_commands(Some(encoder.finish()));
                Ok(())
            }
        });
        methods.add_method_mut("AddTexture", |_lua, this, params: (String, String)| {
            this.use_texture(params.0, params.1, (false, false), true)
                .map(|_| ())
                .map_err(mlua::Error::external)
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
                .map_err(mlua::Error::external)
        });
        methods.add_method_mut(
            "AddSharedTexture",
            |_lua, this, params: (String, String)| {
                this.use_texture(params.0, params.1, (false, false), true)
                    .map(|_| ())
                    .map_err(mlua::Error::external)
            },
        );

        methods.add_method_mut("SetParam", |_, this, params: (String, f32)| {
            this.set_param(params.0.as_str(), params.1);
            Ok(())
        });
        methods.add_method_mut("SetParamVec2", |_, this, params: (String, f32, f32)| {
            this.set_param(params.0.as_str(), vec2(params.1, params.2));
            Ok(())
        });
        methods.add_method_mut(
            "SetParamVec3",
            |_, this, params: (String, f32, f32, f32)| {
                this.set_param(params.0.as_str(), vec3(params.1, params.2, params.3));
                Ok(())
            },
        );
        methods.add_method_mut(
            "SetParamVec4",
            |_, this, params: (String, f32, f32, f32, f32)| {
                this.set_param(
                    params.0.as_str(),
                    vec4(params.1, params.2, params.3, params.4),
                );
                Ok(())
            },
        );

        methods.add_method_mut("SetData", |_lua, this, verts: Vec<LuaVert2>| {
            this.vertex_count = verts.len();
            let (pos, uv): (Vec<_>, Vec<_>) = verts
                .iter()
                .map(|vert| (vec3(vert.0 .0, vert.0 .1, 0.0), vec2(vert.1 .0, vert.1 .1)))
                .unzip();

            this.vertecies_pos = pos;
            this.vertecies_uv = uv;
            this.vertecies_color = None;

            this.update_indecies().map_err(mlua::Error::external)
        });
        methods.add_method_mut("SetBlendMode", |_, this, params: u8| {
            match params {
                0 => this.state = wgpu::BlendState::ALPHA_BLENDING,
                1 => this.state = blend_add(),
                2 => {
                    this.state = wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Dst,
                            dst_factor: wgpu::BlendFactor::Zero,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::OVER,
                    }
                }
                _ => {
                    return Err(mlua::Error::RuntimeError(format!(
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
                    return Err(mlua::Error::RuntimeError(format!(
                        "{params} is not a valid drawing mode."
                    )))
                }
            }

            this.update_indecies().map_err(mlua::Error::external)
        });
    }

    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_function_get("PRIM_TRILIST", |_, _| Ok(0));
        fields.add_field_function_get("PRIM_TRISTRIP", |_, _| Ok(1));
        fields.add_field_function_get("PRIM_TRIFAN", |_, _| Ok(2));

        fields.add_field_function_get::<_, u8>("PRIM_LINELIST", |_, _| lua_todo());
        fields.add_field_function_get::<_, u8>("PRIM_LINESTRIP", |_, _| lua_todo());
        fields.add_field_function_get::<_, u8>("PRIM_POINTLIST", |_, _| lua_todo());

        fields.add_field_function_get("BLEND_NORM", |_, _| Ok(0));
        fields.add_field_function_get("BLEND_ADD", |_, _| Ok(1));
        fields.add_field_function_get("BLEND_MULT", |_, _| Ok(2));
    }
}

fn lua_todo<T>() -> Result<T, mlua::Error> {
    Err(mlua::Error::RuntimeError("Not implemented".into()))
}

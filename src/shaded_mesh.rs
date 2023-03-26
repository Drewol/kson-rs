use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::ensure;
use tealr::{
    mlu::{
        mlua::{FromLua, Lua},
        TealData, UserData,
    },
    TypeName,
};
use three_d::{
    vec2, vec3, vec4, Blend, CpuTexture, ElementBuffer, FrameInput, Mat4, Program, RenderStates,
    Texture2D, Vec2, Vec3, Vec4, VertexBuffer,
};

use crate::vg_ui::Vgfx;

enum ShaderParam {
    Single(f32),
    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),
    Texture(Texture2D),
}

/// https://www.khronos.org/opengl/wiki/Primitive#Triangle_primitives for calculating indecies
enum DrawingMode {
    Triangles,
    Strip,
    Fan,
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
}

impl ShadedMesh {
    pub fn new(
        context: &three_d::Context,
        material: String,
        path: impl AsRef<Path>,
    ) -> anyhow::Result<Self> {
        let mut shader_path = path.as_ref().to_path_buf();
        shader_path.push(material);
        shader_path.set_extension("vs");

        let vertex_shader_source = std::fs::read_to_string(&shader_path)?;
        let fragment_shader_source = std::fs::read_to_string(shader_path.with_extension("fs"))?;

        Ok(Self {
            params: HashMap::new(),
            material: Program::from_source(
                context,
                &vertex_shader_source,
                &fragment_shader_source,
            )?,
            state: RenderStates {
                cull: three_d::Cull::None,
                blend: Blend::STANDARD_TRANSPARENCY,
                depth_test: three_d::DepthTest::Always,
                write_mask: three_d::WriteMask::COLOR,
            },
            vertex_count: 0,
            draw_mode: DrawingMode::Triangles,
            indecies: ElementBuffer::new(context),
            vertecies_pos: VertexBuffer::new(context),
            vertecies_uv: VertexBuffer::new(context),
        })
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

    fn use_texture(
        &mut self,
        context: &three_d::Context,
        name: String,
        path: impl AsRef<Path>,
    ) -> anyhow::Result<()> {
        let texture: CpuTexture = three_d_asset::io::load_and_deserialize(path)?;
        let texture = three_d::Texture2D::new(context, &texture);
        self.material.use_texture(&name, &texture);
        self.params.insert(name, ShaderParam::Texture(texture));
        Ok(())
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
            let frame = &lua.app_data_ref::<FrameInput>().unwrap();
            let vgfx = &lua.app_data_ref::<Arc<Mutex<Vgfx>>>().unwrap();

            let t = {
                let vgfx = vgfx.lock().unwrap();
                let canvas = vgfx.canvas.lock().unwrap();
                canvas.transform().to_mat3x4()
            };

            for (name, param) in &this.params {
                match param {
                    ShaderParam::Single(v) => this.material.use_uniform(name, v),
                    ShaderParam::Vec2(v) => this.material.use_uniform(name, v),
                    ShaderParam::Vec3(v) => this.material.use_uniform(name, v),
                    ShaderParam::Vec4(v) => this.material.use_uniform(name, v),
                    ShaderParam::Texture(v) => this.material.use_texture(name, v),
                }
            }

            this.material.use_uniform(
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

            this.material.use_uniform("world", Mat4::from_scale(1.0));

            this.material
                .use_vertex_attribute("inPos", &this.vertecies_pos); //Vertex positions
            if this.material.requires_attribute("inTex") {
                this.material
                    .use_vertex_attribute("inTex", &this.vertecies_uv); //UVs
            }
            this.material
                .draw_elements(this.state, frame.viewport, &this.indecies);

            Ok(())
        });
        methods.add_method_mut("AddTexture", |lua, this, params: (String, String)| {
            let context = &lua.app_data_ref::<FrameInput>().unwrap().context;
            this.use_texture(context, params.0, params.1)
                .map_err(tealr::mlu::mlua::Error::external)
        });
        methods.add_method_mut("AddSkinTexture", |lua, this, params: (String, String)| {
            let context = &lua.app_data_ref::<FrameInput>().unwrap().context;
            this.use_texture(context, params.0, params.1)
                .map_err(tealr::mlu::mlua::Error::external)
        });
        methods.add_method_mut("AddSharedTexture", |lua, this, params: (String, String)| {
            let context = &lua.app_data_ref::<FrameInput>().unwrap().context;
            this.use_texture(context, params.0, params.1)
                .map_err(tealr::mlu::mlua::Error::external)
        });

        methods.add_method_mut("SetParam", |_, this, params: (String, f32)| {
            this.material.use_uniform(&params.0, params.1);
            this.params.insert(params.0, ShaderParam::Single(params.1));
            Ok(())
        });
        methods.add_method_mut("SetParamVec2", |_, this, params: (String, f32, f32)| {
            let data = vec2(params.1, params.2);
            this.material.use_uniform(&params.0, data);
            this.params.insert(params.0, ShaderParam::Vec2(data));
            Ok(())
        });
        methods.add_method_mut(
            "SetParamVec3",
            |_, this, params: (String, f32, f32, f32)| {
                let data = vec3(params.1, params.2, params.3);
                this.material.use_uniform(&params.0, data);
                this.params.insert(params.0, ShaderParam::Vec3(data));
                Ok(())
            },
        );
        methods.add_method_mut(
            "SetParamVec4",
            |_, this, params: (String, f32, f32, f32, f32)| {
                let data = vec4(params.1, params.2, params.3, params.4);
                this.material.use_uniform(&params.0, data);
                this.params.insert(params.0, ShaderParam::Vec4(data));
                Ok(())
            },
        );

        methods.add_method_mut("SetData", |lua, this, (verts,): (Vec<LuaVert2>,)| {
            this.vertex_count = verts.len();
            let (pos, uv): (Vec<_>, Vec<_>) = verts
                .iter()
                .map(|vert| (vec2(vert.0 .0, vert.0 .1), vec2(vert.1 .0, vert.1 .1)))
                .unzip();

            let context = &lua.app_data_ref::<FrameInput>().unwrap().context;
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

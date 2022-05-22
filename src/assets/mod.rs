use egui_glow::{
    check_for_gl_error,
    glow::{Context, HasContext, Program, Texture},
};
use once_cell::sync::OnceCell;

pub mod textures {
    pub const LASER: &[u8] = include_bytes!("textures/laser.png");
    pub const TRACK: &[u8] = include_bytes!("textures/track.png");
    pub const BT_CHIP: &[u8] = include_bytes!("textures/button.png");
    pub const BT_HOLD: &[u8] = include_bytes!("textures/buttonhold.png");
    pub const FX_CHIP: &[u8] = include_bytes!("textures/fxbutton.png");
    pub const FX_HOLD: &[u8] = include_bytes!("textures/fxbuttonhold.png");
}
#[derive(Debug, Copy, Clone)]
pub struct AssetInstance {
    pub(crate) laser_texture: Texture,
    pub(crate) track_texture: Texture,
    pub(crate) bt_chip_texture: Texture,
    pub(crate) bt_hold_texture: Texture,
    pub(crate) fx_chip_texture: Texture,
    pub(crate) fx_hold_texture: Texture,
    pub(crate) laser_shader: Program,
    pub(crate) track_shader: Program,
    pub(crate) lane_light_shader: Program,
    pub(crate) chip_shader: Program,
    pub(crate) hold_shader: Program,
}
fn load_shader(
    gl: &Context,
    vertex: &str,
    fragment: &str,
) -> Result<egui_glow::glow::NativeProgram, String> {
    use egui_glow::glow;
    unsafe {
        let vert = gl.create_shader(glow::VERTEX_SHADER)?;
        gl.shader_source(vert, vertex);
        gl.compile_shader(vert);

        let frag = gl.create_shader(glow::FRAGMENT_SHADER)?;
        gl.shader_source(frag, fragment);
        gl.compile_shader(frag);

        let program = gl.create_program()?;
        gl.attach_shader(program, vert);
        gl.attach_shader(program, frag);
        gl.link_program(program);

        if gl.get_program_link_status(program) {
            let attribs = gl.get_active_attributes(program);

            log::debug!("Listing attributes");
            for i in 0..attribs {
                if let Some(attrib) = gl.get_active_attribute(program, i) {
                    log::debug!("name: {}, size: {}", attrib.name, attrib.size);
                }
            }

            Ok(program)
        } else {
            Err(gl.get_program_info_log(program))
        }
    }
}

fn load_texture(gl: &Context, texture: &[u8]) -> Result<Texture, String> {
    use egui_glow::glow;
    unsafe {
        let tex = gl.create_texture()?;
        let img = image::load_from_memory_with_format(texture, image::ImageFormat::Png)
            .map_err(|e| format!("{}", e))?;

        gl.bind_texture(glow::TEXTURE_2D, Some(tex));

        let (internal_format, format, ty) = match img {
            image::DynamicImage::ImageLuma8(_) => (glow::R8, glow::RED, glow::UNSIGNED_BYTE),
            image::DynamicImage::ImageLumaA8(_) => (glow::RGBA8, glow::RG, glow::UNSIGNED_BYTE),
            image::DynamicImage::ImageRgb8(_) => (glow::RGB8, glow::RGB, glow::UNSIGNED_BYTE),
            image::DynamicImage::ImageRgba8(_) => (glow::RGBA8, glow::RGBA, glow::UNSIGNED_BYTE),
            f => {
                return Err(format!("Unsupported texture format {:?}", f));
            }
        };
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            internal_format as i32,
            img.width() as i32,
            img.height() as i32,
            0,
            format,
            ty,
            Some(img.as_bytes()),
        );

        gl.generate_mipmap(glow::TEXTURE_2D);

        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR_MIPMAP_LINEAR as i32,
        );

        Ok(tex)
    }
}
static INSTANCE: OnceCell<AssetInstance> = OnceCell::new();

pub fn instance(gl: &Context) -> AssetInstance {
    *INSTANCE
        .get_or_try_init(|| -> Result<AssetInstance, String> {
            log::debug!("Initializing asset instance");
            Ok(AssetInstance {
                laser_texture: load_texture(gl, textures::LASER)?,
                track_texture: load_texture(gl, textures::TRACK)?,
                bt_chip_texture: load_texture(gl, textures::BT_CHIP)?,
                bt_hold_texture: load_texture(gl, textures::BT_HOLD)?,
                fx_chip_texture: load_texture(gl, textures::FX_CHIP)?,
                fx_hold_texture: load_texture(gl, textures::FX_HOLD)?,
                laser_shader: shaders::laser::load(gl)?,
                track_shader: shaders::track::load(gl)?,
                lane_light_shader: shaders::lane_light::load(gl)?,
                chip_shader: shaders::button::load_chip(gl)?,
                hold_shader: shaders::button::load_hold(gl)?,
            })
        })
        .unwrap()
}

pub mod shaders {

    pub mod laser {
        use egui_glow::glow::{Context, Program};

        pub const FRAGMENT: &str = include_str!("shaders/laser_frag.glsl");
        pub const VERTEX: &str = include_str!("shaders/laser_vert.glsl");

        pub fn load(gl: &Context) -> Result<Program, String> {
            super::super::load_shader(gl, VERTEX, FRAGMENT)
        }
    }

    pub mod track {
        use egui_glow::glow::{Context, Program};

        pub const FRAGMENT: &str = include_str!("shaders/track_frag.glsl");
        pub const VERTEX: &str = include_str!("shaders/track_vert.glsl");

        pub fn load(gl: &Context) -> Result<Program, String> {
            super::super::load_shader(gl, VERTEX, FRAGMENT)
        }
    }

    pub mod lane_light {
        use egui_glow::glow::{Context, Program};

        pub const FRAGMENT: &str = include_str!("shaders/lane_light_frag.glsl");
        pub const VERTEX: &str = include_str!("shaders/lane_light_vert.glsl");

        pub fn load(gl: &Context) -> Result<Program, String> {
            super::super::load_shader(gl, VERTEX, FRAGMENT)
        }
    }

    pub mod button {
        use egui_glow::glow::{Context, Program};

        pub const CHIP_FRAGMENT: &str = include_str!("shaders/button_frag.glsl");
        pub const CHIP_VERTEX: &str = include_str!("shaders/button_vert.glsl");
        pub const HOLD_FRAGMENT: &str = include_str!("shaders/holdbutton_frag.glsl");
        pub const HOLD_VERTEX: &str = include_str!("shaders/holdbutton_vert.glsl");

        pub fn load_chip(gl: &Context) -> Result<Program, String> {
            super::super::load_shader(gl, CHIP_VERTEX, CHIP_FRAGMENT)
        }

        pub fn load_hold(gl: &Context) -> Result<Program, String> {
            super::super::load_shader(gl, HOLD_VERTEX, HOLD_FRAGMENT)
        }
    }
}

//! Compatibility layer for legacy skin shaders.
//!
//! Skins written for USC/OpenGL ship GLSL 330 style `.vs`/`.fs` pairs. naga's GLSL frontend only
//! accepts Vulkan flavoured GLSL 450, so the sources are rewritten before parsing:
//! - `#version`/`#extension`/`precision` lines and existing `layout(location=..)` are removed
//! - vertex inputs get locations matching [`crate::shaded_mesh::GpuVertex`] (`inPos`, `inTex`, `inColor`)
//! - varyings get locations matched by name between the stages
//! - non-opaque uniforms are wrapped in their own uniform block in bind group 0
//! - `sampler2D` uniforms are split into a `texture2D` + `sampler` pair in their own bind group,
//!   uses are replaced with `sampler2D(tex, tex_s)`
//! - `texture2D()` and `gl_FragColor` are replaced with their modern equivalents

use std::collections::{BTreeMap, HashMap};

use anyhow::{anyhow, Context};
use regex::Regex;
use wgpu::naga;

/// Prefix of the generated uniform blocks, stripped again after parsing so uniforms can be found by name
const BLOCK_PREFIX: &str = "rusc_U_";
/// Prefix of renamed textures, avoids clashes with local variables of the same name
const TEXTURE_PREFIX: &str = "rusc_T_";
const FRAG_COLOR: &str = "rusc_FragColor";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Vertex,
    Fragment,
}

#[derive(Default)]
struct Bindings {
    /// Non-opaque uniforms -> binding in group 0
    uniforms: HashMap<String, u32>,
    /// Texture uniforms -> bind group
    textures: HashMap<String, u32>,
    /// Varyings -> location
    varyings: HashMap<String, u32>,
    /// Vertex inputs -> location
    attributes: HashMap<String, u32>,
}

impl Bindings {
    fn uniform(&mut self, name: &str) -> u32 {
        let next = self.uniforms.len() as u32;
        *self.uniforms.entry(name.to_string()).or_insert(next)
    }

    fn texture(&mut self, name: &str) -> u32 {
        let next = self.textures.len() as u32 + 1;
        *self.textures.entry(name.to_string()).or_insert(next)
    }

    fn varying(&mut self, name: &str) -> u32 {
        let next = self.varyings.len() as u32;
        *self.varyings.entry(name.to_string()).or_insert(next)
    }

    fn attribute(&mut self, name: &str) -> u32 {
        match name {
            "inPos" => 0,
            "inTex" => 1,
            "inColor" => 2,
            _ => {
                let next = self.attributes.len() as u32 + 3;
                *self.attributes.entry(name.to_string()).or_insert(next)
            }
        }
    }
}

struct Regexes {
    strip_layout: Regex,
    declaration: Regex,
    texture2d: Regex,
    frag_color: Regex,
}

fn regexes() -> &'static Regexes {
    static RE: std::sync::OnceLock<Regexes> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regexes {
        strip_layout: Regex::new(r"layout\s*\([^)]*\)\s*").unwrap(),
        declaration: Regex::new(
            r"^(?P<indent>\s*)(?P<storage>uniform|in|out|attribute|varying)\s+(?P<quals>(?:(?:flat|smooth|noperspective|highp|mediump|lowp)\s+)*)(?P<ty>\w+)\s+(?P<names>[\w\s,\[\]]+?)\s*;(?P<rest>.*)$",
        )
        .unwrap(),
        texture2d: Regex::new(r"\btexture2D\s*\(").unwrap(),
        frag_color: Regex::new(r"\bgl_FragColor\b").unwrap(),
    })
}

fn is_sampler(ty: &str) -> Option<&'static str> {
    match ty {
        "sampler2D" => Some("texture2D"),
        _ => None,
    }
}

/// Splits `a, b[2]` into `[("a", ""), ("b", "[2]")]`
fn split_names(names: &str) -> Vec<(String, String)> {
    names
        .split(',')
        .map(|x| x.trim())
        .filter(|x| !x.is_empty())
        .map(|x| match x.find('[') {
            Some(i) => (x[..i].trim().to_string(), x[i..].replace(' ', "")),
            None => (x.to_string(), String::new()),
        })
        .collect()
}

fn preprocess(source: &str) -> String {
    let re = regexes();
    let source = re.strip_layout.replace_all(source, "");
    source
        .lines()
        .filter(|x| {
            let x = x.trim_start();
            !x.starts_with("#version") && !x.starts_with("#extension") && !x.starts_with("precision")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// First pass: assign bindings and locations so both stages agree
fn collect(source: &str, stage: Stage, bindings: &mut Bindings) {
    let re = regexes();
    for line in source.lines() {
        let Some(c) = re.declaration.captures(line) else {
            continue;
        };
        for (name, _) in split_names(&c["names"]) {
            match (&c["storage"], stage) {
                ("uniform", _) if is_sampler(&c["ty"]).is_some() => {
                    bindings.texture(&name);
                }
                ("uniform", _) => {
                    bindings.uniform(&name);
                }
                ("out" | "varying", Stage::Vertex) | ("in" | "varying", Stage::Fragment) => {
                    bindings.varying(&name);
                }
                _ => {}
            }
        }
    }
}

fn rewrite(source: &str, stage: Stage, bindings: &mut Bindings) -> String {
    let re = regexes();
    let mut out = String::with_capacity(source.len() * 2);
    out.push_str("#version 450\n");

    let mut frag_out_location = 0;
    let uses_frag_color = stage == Stage::Fragment && re.frag_color.is_match(source);
    if uses_frag_color {
        out.push_str(&format!("layout(location = 0) out vec4 {FRAG_COLOR};\n"));
        frag_out_location = 1;
    }

    let mut textures_used = vec![];
    let mut body = String::with_capacity(source.len());

    for line in source.lines() {
        let Some(c) = re.declaration.captures(line) else {
            body.push_str(line);
            body.push('\n');
            continue;
        };

        let indent = &c["indent"];
        let quals = &c["quals"];
        let ty = &c["ty"];
        let rest = &c["rest"];
        let storage = &c["storage"];

        for (name, array) in split_names(&c["names"]) {
            let decl = match (storage, stage) {
                ("uniform", _) => {
                    if let Some(tex_ty) = is_sampler(ty) {
                        let group = bindings.texture(&name);
                        textures_used.push(name.clone());
                        format!(
                            "layout(set = {group}, binding = 0) uniform {tex_ty} {TEXTURE_PREFIX}{name}; \
                             layout(set = {group}, binding = 1) uniform sampler {TEXTURE_PREFIX}{name}_s;"
                        )
                    } else if ty == "bool" {
                        // bool is not host shareable, use an int instead
                        let binding = bindings.uniform(&name);
                        format!(
                            "layout(std140, set = 0, binding = {binding}) uniform {BLOCK_PREFIX}{name} {{ int rusc_bool_{name}{array}; }};\n#define {name} (rusc_bool_{name} != 0)"
                        )
                    } else {
                        let binding = bindings.uniform(&name);
                        format!(
                            "layout(std140, set = 0, binding = {binding}) uniform {BLOCK_PREFIX}{name} {{ {quals}{ty} {name}{array}; }};"
                        )
                    }
                }
                ("in" | "attribute", Stage::Vertex) => {
                    let location = bindings.attribute(&name);
                    format!("layout(location = {location}) in {quals}{ty} {name}{array};")
                }
                ("out" | "varying", Stage::Vertex) => {
                    let location = bindings.varying(&name);
                    format!("layout(location = {location}) out {quals}{ty} {name}{array};")
                }
                ("in" | "varying", Stage::Fragment) => {
                    let location = bindings.varying(&name);
                    format!("layout(location = {location}) in {quals}{ty} {name}{array};")
                }
                ("out", Stage::Fragment) => {
                    let location = frag_out_location;
                    frag_out_location += 1;
                    format!("layout(location = {location}) out {quals}{ty} {name}{array};")
                }
                ("attribute", Stage::Fragment) => continue,
                _ => unreachable!(),
            };
            body.push_str(indent);
            body.push_str(&decl);
        }
        body.push_str(rest);
        body.push('\n');
    }

    let mut body = re.texture2d.replace_all(&body, "texture(").into_owned();
    if uses_frag_color {
        body = re.frag_color.replace_all(&body, FRAG_COLOR).into_owned();
    }

    // Combine the separated texture and sampler where the texture is passed to a function
    for tex in textures_used.iter() {
        let use_re = Regex::new(&format!(r"([(,]\s*){}(\s*[,)])", regex::escape(tex)))
            .expect("Bad regex");
        let replacement = format!("${{1}}sampler2D({TEXTURE_PREFIX}{tex}, {TEXTURE_PREFIX}{tex}_s)${{2}}");
        // Run twice to handle adjacent matches sharing a delimiter, e.g. `f(tex,tex)`
        for _ in 0..2 {
            body = use_re.replace_all(&body, replacement.as_str()).into_owned();
        }
    }

    out.push_str(&body);
    out
}

/// Rewrites a legacy GLSL shader pair into GLSL 450 that naga can parse
pub fn translate(vertex: &str, fragment: &str) -> (String, String) {
    let vertex = preprocess(vertex);
    let fragment = preprocess(fragment);
    let mut bindings = Bindings::default();
    collect(&vertex, Stage::Vertex, &mut bindings);
    collect(&fragment, Stage::Fragment, &mut bindings);

    (
        rewrite(&vertex, Stage::Vertex, &mut bindings),
        rewrite(&fragment, Stage::Fragment, &mut bindings),
    )
}

fn parse_stage(source: &str, stage: naga::ShaderStage) -> anyhow::Result<naga::Module> {
    let mut module = naga::front::glsl::Frontend::default()
        .parse(&naga::front::glsl::Options::from(stage), source)
        .map_err(|e| anyhow!("{}", e.emit_to_string(source)))?;

    // Name block uniforms after their only member so they can be set by name
    let renamed: BTreeMap<_, _> = module
        .global_variables
        .iter()
        .filter_map(|(handle, global)| {
            let ty = &module.types[global.ty];
            if let Some(name) = global.name.as_deref() {
                if let Some(stripped) = name.strip_prefix(TEXTURE_PREFIX) {
                    return Some((handle, stripped.to_string()));
                }
            }
            let name = global.name.as_deref().or(ty.name.as_deref())?;
            let stripped = name.strip_prefix(BLOCK_PREFIX)?;
            Some((handle, stripped.to_string()))
        })
        .collect();

    for (handle, name) in renamed {
        module.global_variables.get_mut(handle).name = Some(name);
    }

    Ok(module)
}

/// Translates and parses a legacy GLSL shader pair into naga modules
pub fn parse(vertex: &str, fragment: &str) -> anyhow::Result<(naga::Module, naga::Module)> {
    let (vertex, fragment) = translate(vertex, fragment);
    let vs = parse_stage(&vertex, naga::ShaderStage::Vertex).context("Vertex shader")?;
    let fs = parse_stage(&fragment, naga::ShaderStage::Fragment).context("Fragment shader")?;
    Ok((vs, fs))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    fn validate(module: &wgpu::naga::Module) -> anyhow::Result<()> {
        use wgpu::naga::valid::*;
        Validator::new(ValidationFlags::all(), Capabilities::all())
            .validate(module)
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("{e:?}"))
    }

    fn check_pair(vs: &Path, fs: &Path) -> anyhow::Result<()> {
        let vs = std::fs::read_to_string(vs)?;
        let fs = std::fs::read_to_string(fs)?;
        let (v, f) = super::parse(&vs, &fs)?;
        validate(&v)?;
        validate(&f)?;
        Ok(())
    }

    #[test]
    fn default_skin_wgsl_shaders() {
        let shaders = Path::new(env!("CARGO_MANIFEST_DIR")).join("skins/Default/shaders");
        let mut failed = vec![];
        for entry in std::fs::read_dir(&shaders).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|x| x != "wgsl") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let result = wgpu::naga::front::wgsl::parse_str(&source)
                .map_err(|e| anyhow::anyhow!("{}", e.emit_to_string(&source)))
                .and_then(|m| validate(&m));
            if let Err(e) = result {
                failed.push(format!("{path:?}: {e:#}"));
            }
        }
        assert!(failed.is_empty(), "{}", failed.join("\n\n"));
    }

    #[test]
    fn default_skin_shaders() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let shaders = root.join("skins/Default/shaders");
        let mut failed = vec![];
        let mut checked = 0;
        for entry in std::fs::read_dir(&shaders).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|x| x == "vs") {
                let fs = path.with_extension("fs");
                // Geometry shaders are not supported
                if !fs.exists() || path.with_extension("gs").exists() {
                    continue;
                }
                checked += 1;
                if let Err(e) = check_pair(&path, &fs) {
                    failed.push(format!("{path:?}: {e:#}"));
                }
            }
        }

        let fullscreen = root.join("src/static_assets/fullscreen.vs");
        for entry in std::fs::read_dir(root.join("skins/Default/backgrounds")).unwrap() {
            let dir = entry.unwrap().path();
            for name in ["bg.fs", "fg.fs"] {
                let fs = dir.join(name);
                if fs.exists() {
                    checked += 1;
                    if let Err(e) = check_pair(&fullscreen, &fs) {
                        failed.push(format!("{fs:?}: {e:#}"));
                    }
                }
            }
        }

        assert!(checked > 0);
        assert!(failed.is_empty(), "{}", failed.join("\n\n"));
    }
}

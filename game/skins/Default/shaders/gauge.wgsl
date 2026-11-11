@group(0) @binding(0) var<uniform> proj: mat4x4f;
@group(0) @binding(1) var<uniform> world: mat4x4f;
@group(0) @binding(2) var<uniform> time: f32;
@group(0) @binding(3) var<uniform> rate: f32;
@group(0) @binding(4) var<uniform> barColor: vec4f;

@group(1) @binding(0) var mainTex: texture_2d<f32>;
@group(1) @binding(1) var mainTex_s: sampler;

@group(2) @binding(0) var maskTex: texture_2d<f32>;
@group(2) @binding(1) var maskTex_s: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3f, @location(1) tex: vec2f) -> VertexOutput {
    var res: VertexOutput;
    res.tex_coords = tex;
    res.position = proj * world * vec4(position.xy, 0.0, 1.0);
    return res;
}

@fragment
fn fs_main(v: VertexOutput) -> @location(0) vec4f {
    let tex = textureSample(mainTex, mainTex_s, v.tex_coords);
    var mask = textureSample(maskTex, maskTex_s, v.tex_coords).x;
    mask = clamp((rate - mask) * 100.0, 0.0, 1.0);
    return vec4(tex.rgb * barColor.rgb, tex.a * mask);
}

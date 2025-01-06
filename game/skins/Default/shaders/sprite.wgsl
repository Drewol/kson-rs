@group(0) @binding(0) var<uniform> proj: mat4x4f; 
@group(0) @binding(1) var<uniform> camera: mat4x4f; 
@group(0) @binding(2) var<uniform> world: mat4x4f; 

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
};


@vertex
fn vs_main(
    @location(0) position: vec3f, @location(1) tex: vec2f
) -> VertexOutput {
    var res: VertexOutput;
    res.position = proj * camera * world * vec4(position, 1.0);
    res.tex_coords = tex;
    return res;

}

@group(1) @binding(0) var mainTex: texture_2d<f32>; 
@group(1) @binding(1) var mainTex_s: sampler; 

@group(2) @binding(0) var<uniform> color: vec4f;

@fragment
fn fs_main(v: VertexOutput) -> @location(0) vec4f {
        let mainColor = textureSample(mainTex, mainTex_s, v.tex_coords);
        return mainColor * color;
}
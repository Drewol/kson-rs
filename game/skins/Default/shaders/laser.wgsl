@group(1) @binding(0) var mainTex: texture_2d<f32>; 
@group(1) @binding(1) var mainTex_s: sampler; 

@group(0) @binding(0) var<uniform> proj: mat4x4f; 
@group(0) @binding(1) var<uniform> camera: mat4x4f; 
@group(0) @binding(2) var<uniform> world: mat4x4f; 

@group(2) @binding(0) var<uniform> color: vec4f;

@group(2) @binding(1) var<uniform> objectGlow: f32;
@group(2) @binding(2) var<uniform> trackPos: f32;
@group(2) @binding(3) var<uniform> trackScale: f32;
@group(2) @binding(4) var<uniform> hiddenCutoff: f32;
@group(2) @binding(5) var<uniform> hiddenFadeWindow: f32;
@group(2) @binding(6) var<uniform> suddenCutoff: f32;
@group(2) @binding(7) var<uniform> suddenFadeWindow: f32;

// 20Hz flickering. 0 = Miss, 1 = Inactive, 2 & 3 = Active alternating.
@group(2) @binding(8) var<uniform> hitState: i32;

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

@fragment
fn fs_main(v: VertexOutput) -> @location(0) vec4<f32> {
    var main_color = textureSample(mainTex, mainTex_s, v.tex_coords);
    
    main_color = main_color * (vec4<f32>(vec3(1.0 + objectGlow * 0.3), 1.0));
    
    main_color.a = min(1.0, main_color.a + main_color.a * objectGlow * 0.9);
    
    //main_color *= hide(v.position);
    
    return main_color * color;
}
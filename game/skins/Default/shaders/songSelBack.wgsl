@group(0) @binding(0) var<uniform> scale: f32;
@group(0) @binding(1) var<uniform> timer: f32;
@group(0) @binding(2) var<uniform> seed: f32;
@group(0) @binding(3) var<uniform> color: vec4f;

@group(1) @binding(0) var<uniform> proj: mat4x4f;
@group(1) @binding(1) var<uniform> world: mat4x4f;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
};


@vertex
fn vs_main(@location(0) position: vec3f, @location(1) tex: vec2f) -> VertexOutput {   
    var result: VertexOutput;
    let seeded = position.xy + vec2<f32>(seed);
    let rand = fract(sin(dot(seeded.xy, vec2<f32>(12.9898, 78.233))) * 43758.5453123);
    let rand2 = fract(sin(dot(seeded.xy * 2.0, vec2<f32>(12.9898, 78.233))) * 43758.5453123);
    let rand3 = fract(sin(dot(seeded.xy * 3.0, vec2<f32>(12.9898, 78.233))) * 43758.5453123);

    let fs_tex = vec2<f32>(pow(rand3, 0.5));

    let offscale = scale / 1920.0;
    let size = 65.0 * offscale;
    let offset = vec2<f32>(
        cos(rand + timer * rand3 * 1.8),
        sin(rand + timer * rand2 * 2.0)
    ) * 0.05;

    
    result.position = vec4(position.xy * 2.25 + offset + vec2(-1.2, -1.2), 0.0, 1.0);
    result.tex_coords = fs_tex;
    return result;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
	return color * pow(length(input.tex_coords), 2.0) * 0.8;
    
}


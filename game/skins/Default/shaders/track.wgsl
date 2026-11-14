struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
};

@group(0) @binding(0) var<uniform> proj: mat4x4f; 
@group(0) @binding(1) var<uniform> camera: mat4x4f; 
@group(0) @binding(2) var<uniform> world: mat4x4f; 
@group(0) @binding(3) var<uniform> hidden: f32; 
@group(0) @binding(4) var<uniform> rCol: vec4f; 
@group(0) @binding(5) var<uniform> lCol: vec4f; 

@group(2) @binding(0)
var main_tex: texture_2d<f32>;
@group(2) @binding(1)
var main_tex_s: sampler;

@group(1) @binding(0)
var<uniform> transform: mat4x4f;


@vertex
fn vs_main(@location(0) position: vec3f, @location(1) tex: vec2f) -> VertexOutput {
    var res: VertexOutput;
    res.position = proj * camera * world * vec4(position, 1.0);
    res.tex_coords = tex;
    return res;
}


@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let mainColor = textureSample(main_tex, main_tex_s, input.tex_coords);
    var col = mainColor.xyz;
    var alpha = mainColor.a;

    if(input.tex_coords.y > hidden)
    {
        //Red channel to color right lane
        col = vec3(.9) * rCol.xyz * vec3(mainColor.x);

        //Blue channel to color left lane
        col += vec3(.9) * lCol.xyz * vec3(mainColor.z);

        //Color green channel white
        col += vec3(.6) * vec3(mainColor.y);
    }
    else
    {
        col = vec3(0.);
        alpha = select(0.0, 0.3, alpha > 0.0);
    }
    return vec4(col, alpha);
}
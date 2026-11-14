const v: array<vec2f, 3> = array(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>(3.0, -1.0),
    vec2<f32>(-1.0, 3.0)
);


@group(0) @binding(0) var<uniform> viewport: vec2u; 
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index)index: u32) -> VertexOutput {
    var r: VertexOutput;
    
    // Single triangle covering the whole viewport: (-1,-1), (3,-1), (-1,3)
    var vertex: vec2<f32> = vec2(
        f32((index << 1u) & 2u) * 2.0 - 1.0,
        f32(index & 2u) * 2.0 - 1.0
        );
    
    var tex_vp = 0.5 * vertex + vec2<f32>(0.5);
    tex_vp.x *= f32(viewport.x);
    tex_vp.y *= f32(viewport.y);
    r.position =  vec4(vertex, 0, 1);
    r.tex_coords = tex_vp;


    return r;
}
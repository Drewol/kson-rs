// Fragment shader for `ShadedMesh::new_fullscreen_tex`, appended to fullscreen.wgsl
// Copies `mainTex` 1:1 to the render target, `mainTex` is expected to be the same size as the target.

@group(1) @binding(0) var mainTex: texture_2d<f32>;
@group(1) @binding(1) var mainTex_s: sampler;

@fragment
fn fs_main(v: VertexOutput) -> @location(0) vec4<f32> {
    return textureLoad(mainTex, vec2<i32>(floor(v.position.xy)), 0);
}

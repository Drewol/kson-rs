@vertex
fn vs_main() -> @builtin(position) vec4f {
    return vec4(0.0);
}

@group(0) @binding(0) var mainTex: texture_2d<f32>; 
@group(0) @binding(1) var mainTex_s: sampler; 

@fragment
fn fs_main() {}
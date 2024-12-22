@group(0) @binding(0) var mainTex: texture_2d<f32>; 
@group(0) @binding(1) var mainTex_s: sampler; 

@group(1) @binding(0) var maskTex: texture_2d<f32>; 
@group(2) @binding(1) var maskTex_s: sampler; 


@vertex
fn vs_main() -> @builtin(position) vec4f {
    return vec4(0.0);
}


@fragment
fn fs_main() {}
@group(0) @binding(0) var<uniform> proj: mat4x4f; 
@group(0) @binding(1) var<uniform> camera: mat4x4f; 
@group(0) @binding(2) var<uniform> world: mat4x4f; 

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    // Per note data, set by the game
    @location(1) data: vec4<f32>,
};


@vertex
fn vs_main(
    @location(0) position: vec3f, @location(1) tex: vec2f, @location(2) data: vec4f
) -> VertexOutput {
    var res: VertexOutput;
    res.position = proj * camera * world * vec4(position, 1.0);
    res.tex_coords = tex;
    res.data = data;
    return res;

}

@group(1) @binding(0) var mainTex: texture_2d<f32>; 
@group(1) @binding(1) var mainTex_s: sampler; 

// data.x: 1.0 if the chip has a key sound
@group(2) @binding(0) var<uniform> trackPos: f32;
@group(2) @binding(1) var<uniform> trackScale: f32;
@group(2) @binding(2) var<uniform> hiddenCutoff: f32;
@group(2) @binding(3) var<uniform> hiddenFadeWindow: f32;
@group(2) @binding(4) var<uniform> suddenCutoff: f32;
@group(2) @binding(5) var<uniform> suddenFadeWindow: f32;

// The OpenGL standard leave the case when `edge0 >= edge1` undefined,
// so this function was made to remove the ambiguity when `edge0 >= edge1`.
// Note that the case when `edge0 > edge1` should be avoided.
fn smoothstep_fix(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge0 >= edge1 {
        return select( 0.0, select( 1.0, 0.5, x > edge1), x < edge0);
    }
   return smoothstep(edge0, edge1, x);
}

fn hide(position: vec4f) -> f32 {
    let off = trackPos + position.y * trackScale;
    let sudden_cutoff = suddenCutoff;
    let hidden_cutoff = hiddenCutoff;
    let sudden_window = suddenFadeWindow;
    let hidden_window = hiddenFadeWindow;

    if hidden_cutoff > sudden_cutoff {
        let sudden = 1.0 - smoothstep_fix(sudden_cutoff - sudden_window, sudden_cutoff, off);
        let hidden = smoothstep_fix(hidden_cutoff, hidden_cutoff + hidden_window, off);
        return min(hidden + sudden, 1.0);
    }

    let sudden = 1.0 - smoothstep_fix(sudden_cutoff, sudden_cutoff + sudden_window, off);
    let hidden = smoothstep_fix(hidden_cutoff - hidden_window, hidden_cutoff, off);

    return hidden * sudden;
}


@fragment
fn fs_main(v: VertexOutput) -> @location(0) vec4f {
    var main_color = textureSample(mainTex, mainTex_s, v.tex_coords);
    
    if(v.data.x > 0.5)
    {
        var addition = abs(0.5 - v.tex_coords.x) * - 1.;
        addition += 0.2;
        addition = max(addition,0.);
        addition *= 2.8;
        main_color += vec4(addition);
    }

    //main_color *= hide(v.position);
    
    return main_color;
}

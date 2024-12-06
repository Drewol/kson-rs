/*
#ifdef EMBEDDED
varying vec2 fsTex;
#else

in vec2 fsTex;
out vec4 target;
#endif

layout(binding=0) uniform int mainTex;
layout(binding=1) uniform vec4 lCol;
layout(binding=2) uniform vec4 rCol;
layout(binding=3) uniform float hidden;

void main()
{	
	vec4 mainColor = texture(mainTex, fsTex.xy);
    vec4 col = mainColor;

    if(fsTex.y > hidden)
    {
        //Red channel to color right lane
        col.xyz = vec3(.9) * rCol.xyz * vec3(mainColor.x);

        //Blue channel to color left lane
        col.xyz += vec3(.9) * lCol.xyz * vec3(mainColor.z);

        //Color green channel white
        col.xyz += vec3(.6) * vec3(mainColor.y);
    }
    else
    {
        col.xyz = vec3(0.);
        col.a = col.a > 0.0 ? 0.3 : 0.0;
    }
    target = col;
}
*/


struct FragInput {
    left_color: vec4<f32>,
    right_color: vec4<f32>,
    hidden: f32,
};

struct CameraUniform {
    view_proj: mat4x4<f32>,
};


@group(0) @binding(0)
var<uniform> fi : FragInput;

@group(0) @binding(1)
var main_tex: texture_2d<f32>;
@group(0) @binding(2)
var tex_s: sampler;

@group(1000) @binding(0)
var<uniform> transform: mat4x4f;


@fragment
fn fs_main(@location(0) fsTex: vec2f) -> @location(0) vec4<f32> {
    let mainColor = textureSample(main_tex, tex_s, fsTex.xy);
    var col = mainColor.xyz;
    var alpha = mainColor.a;

    if(fsTex.y > fi.hidden)
    {
        //Red channel to color right lane
        col = vec3(.9) * fi.right_color.xyz * vec3(mainColor.x);

        //Blue channel to color left lane
        col += vec3(.9) * fi.left_color.xyz * vec3(mainColor.z);

        //Color green channel white
        col += vec3(.6) * vec3(mainColor.y);
    }
    else
    {
        col = vec3(0.);
        alpha = step(alpha, 0.0) * 0.3;
    }
    return vec4f(col, alpha);
}
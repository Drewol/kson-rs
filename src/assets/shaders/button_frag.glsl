#version 100
precision mediump float;
varying lowp vec2 uv;
varying lowp vec4 color;

uniform sampler2D mainTex;
uniform int hasSample;

void main()
{	
	vec4 mainColor = texture2D(mainTex, uv.yx);
    float addition = abs(0.5 - uv.x) * - 1.;
    addition += 0.2;
    addition = max(addition,0.);
    addition *= 2.8;
    mainColor.xyzw += addition * float(hasSample);
    gl_FragColor = mainColor;
}

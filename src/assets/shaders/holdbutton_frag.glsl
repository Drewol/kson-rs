#version 100
precision mediump float;

varying vec2 uv;

uniform sampler2D mainTex;
uniform float objectGlow;
// 20Hz flickering. 0 = Miss, 1 = Inactive, 2 & 3 = Active alternating.
uniform int hitState;


void main()
{    
    vec4 mainColor = texture2D(mainTex, uv.xy);

    vec4 target = mainColor;
	target.xyz = target.xyz * (1.0 + objectGlow * 0.3);
    target.a = min(1.0, target.a + target.a * objectGlow * 0.9);
    gl_FragColor = target;
}


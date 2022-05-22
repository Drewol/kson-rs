#version 100
precision mediump float;
varying vec2 fsTex;
varying vec4 uv;

uniform sampler2D mainTex;
uniform vec3 color;

// 20Hz flickering. 0 = Miss, 1 = Inactive, 2 & 3 = Active alternating.
uniform int state;


void main()
{    
    float x = fsTex.x;
    float laserSize = 0.85; //0.0 to 1.0
    x -= 0.5;
    x /= laserSize;
    x += 0.5;
    vec4 mainColor = texture2D(mainTex, vec2(x,fsTex.y)) * step(0.0, x) * (1.0 - step(1.0, x));

    float brightness = (3.5 / 4.0) + float(state - 1) / 4.0;
    

    gl_FragColor = (mainColor * vec4(color, 1)) * brightness;
}

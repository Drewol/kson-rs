#version 100
precision mediump float;
varying lowp vec2 uv;
varying lowp vec4 color;

uniform sampler2D mainTex;
uniform vec4 lCol;
uniform vec4 rCol;

void main()
{	
	vec4 mainColor = texture2D(mainTex, uv.xy);
    vec4 col = mainColor;
    //Red channel to color right lane
    col.xyz = vec3(.9) * rCol.xyz * vec3(mainColor.x);

    //Blue channel to color left lane
    col.xyz += vec3(.9) * lCol.xyz * vec3(mainColor.z);

    //Color green channel white
    col.xyz += vec3(.6) * vec3(mainColor.y);

    col.xyz += vec3(.002);

    gl_FragColor = col;
}
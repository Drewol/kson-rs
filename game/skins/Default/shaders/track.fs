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
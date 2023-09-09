#ifdef EMBEDDED
varying vec2 fsTex;
#else

in vec2 fsTex;
out vec4 target;
#endif


uniform sampler2D mainTex;
uniform vec4 color;

void main()
{	
	vec4 mainColor = texture(mainTex, fsTex.xy);
	target = mainColor * color;
}
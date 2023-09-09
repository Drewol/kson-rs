#ifdef EMBEDDED
attribute vec2 inPos;
attribute vec2 inTex;
varying vec2 fsTex;
#else

in vec2 inPos;
in vec2 inTex;

out vec2 fsTex;
#endif

uniform mat4 proj;
uniform mat4 camera;
uniform mat4 world;

void main()
{
	fsTex = inTex;
	gl_Position = proj * camera * world * vec4(inPos.xy, 0, 1);
}
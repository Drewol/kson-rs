#ifdef EMBEDDED
attribute vec2 inPos;
attribute vec2 inTex;
varying vec2 fsTex;
varying vec4 position;

#else

in vec2 inPos;
in vec2 inTex;

out gl_PerVertex
{
	vec4 gl_Position;
};
out vec2 fsTex;
out vec4 position;
#endif

uniform mat4 proj;
uniform mat4 camera;
uniform mat4 world;

void main()
{
	fsTex = inTex;

	position = vec4(inPos.xy, 0, 1);

	gl_Position = proj * camera * world * vec4(inPos.xy, 0, 1);
}

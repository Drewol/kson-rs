#ifdef EMBEDDED
varying vec2 fsTex;
#else

in vec2 fsTex;
out vec4 target;
#endif

uniform vec4 color;

void main()
{
	target = color * pow(length(fsTex), 2.0) * 0.8;
}
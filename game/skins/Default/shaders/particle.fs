

in vec4 fsColor;
in vec2 fsTex;
out vec4 target;

uniform sampler2D mainTex;
		
void main()
{
	target = fsColor * texture(mainTex, fsTex);
}
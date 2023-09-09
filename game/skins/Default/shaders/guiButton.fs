#version 330


in vec2 fsTex;
out vec4 target;

uniform sampler2D mainTex;
uniform vec4 color;

void main()
{
	target = texture(mainTex, fsTex) * color;
}
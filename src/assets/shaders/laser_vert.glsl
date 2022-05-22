#version 100
precision mediump float;
attribute vec2 position;
attribute vec2 texcoord;
attribute vec4 color0;

uniform float offset;
uniform mat4 Model;
uniform mat4 Projection;
uniform float scale;

varying vec2 fsTex;
varying vec4 realPos;

void main()
{
	fsTex = texcoord;
	float y = (position.x + offset) / scale;

	realPos = vec4(vec3(y, 0, position.x), 1);

	gl_Position = Projection * realPos * Model;
}

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

void main()
{
	fsTex = texcoord;
	gl_Position = Projection * Model * vec4(position.x, 0,  position.y, 1);
}

#version 100
precision mediump float;
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;

varying lowp vec2 uv;
varying lowp vec4 color;

uniform mat4 Model;
uniform mat4 Projection;

void main() {
	gl_Position = Projection * Model * vec4(position, 1);
	color = color0 / 255.0;
	uv = texcoord;
}
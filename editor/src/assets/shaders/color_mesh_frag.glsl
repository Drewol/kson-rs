#version 100
precision mediump float;

varying lowp vec2 uv;
varying lowp vec4 color;

uniform float brightness;

void main() {
    gl_FragColor = color;
}
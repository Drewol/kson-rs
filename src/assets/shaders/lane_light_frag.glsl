#version 100
precision mediump float;

varying lowp vec2 uv;
varying lowp vec4 color;

uniform float brightness;

void main() {
    gl_FragColor = color * brightness * clamp((1.0 - uv.x) * 2.0 - 0.5, 0.0, 1.0);
}
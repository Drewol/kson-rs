out vec2 texVp; // texcoords are in the normalized [0,1] range for the viewport-filling quad part of the triangle
in vec3 inPos;
uniform ivec2 viewport;
void main() {
    vec2 vertices[3]=vec2[3](vec2(-1,-1), vec2(3,-1), vec2(-1, 3));
    gl_Position = vec4(vertices[int(inPos.x)],0,1);
    texVp = 0.5 * gl_Position.xy + vec2(0.5);
    texVp.x *= float(viewport.x);
    texVp.y *= float(viewport.y);
}

in vec3 inPos;
in vec4 inColor;
in vec4 inParams;

out gl_PerVertex
{
	vec4 gl_Position;
};
out vec4 fsColor;
out vec4 fsParams;

void main()
{
	fsColor = inColor;
	fsParams = inParams;
	gl_Position = vec4(inPos,1);
}
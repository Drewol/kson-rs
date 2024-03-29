#ifdef EMBEDDED
varying vec2 fsTex;
#else

in vec2 fsTex;
out vec4 target;
#endif

uniform float time;
uniform float rate;
uniform sampler2D mainTex;
uniform sampler2D maskTex;
uniform vec4 barColor;

void main()
{
	vec4 tex = texture(mainTex, fsTex);
    float mask = texture(maskTex, fsTex).x;
    mask = rate - mask;
    mask *= 100.0;
    mask = clamp(mask, 0.0, 1.0);
	target.rgb = tex.rgb * barColor.rgb;
    target.a = tex.a * mask;
}
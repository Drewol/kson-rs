#ifdef EMBEDDED
varying vec2 fsTex;
varying vec4 position;
#else

in vec2 fsTex;
out vec4 target;
in vec4 position;
#endif

uniform sampler2D mainTex;

void main()
{	
    float x = fsTex.x;
    if (x < 0.0 || x > 1.0)
    {
        target = vec4(0);
        return;
    }
    float laserSize = 1.0; //0.0 to 1.0
    x -= 0.5;
    x /= laserSize;
    x += 0.5;
	vec4 mainColor = texture(mainTex, vec2(x,fsTex.y));
    target = vec4(0.0, 0.0, 0.0, mainColor.a);
}
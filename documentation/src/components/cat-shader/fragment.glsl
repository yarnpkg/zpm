#version 300 es
precision highp float;
precision highp int;

uniform sampler2D tex;
uniform sampler2D distanceField;
uniform sampler2D stars;

uniform float time;
uniform vec2 resolution;
uniform vec2 textureSize;
uniform float falloff;
uniform float fbmsubstract;

in vec2 v_texCoord;
out vec4 fragColor;

const vec3 color1 = vec3(0.1, 0.65, 1.1);
const vec3 bg = vec3(0.046875, 0.0546875, 0.07421875);

vec2 hash(vec2 p) {
  uvec2 q = uvec2(ivec2(p)) * uvec2(1597334673U, 3812015801U);
  q = (q.x ^ q.y) * uvec2(1597334673U, 3812015801U);
  return vec2(q) * (1.0 / float(0xffffffffU));
}


vec2 noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);

  vec2 a = hash(i);
  vec2 b = hash(i + vec2(1.0, 0.0));
  vec2 c = hash(i + vec2(0.0, 1.0));
  vec2 d = hash(i + vec2(1.0, 1.0));

  vec2 u = f * f * (3.0 - 2.0 * f);

  return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

vec2 fbm(vec2 p) {
  vec2 value = vec2(0.0);
  float amplitude = 0.5;
  float frequency = 1.0;

  for (int i = 0; i < 3; i++) {
    value += amplitude * noise(p * frequency - time * 0.5);
    amplitude *= 0.5;
    frequency *= 2.0;
  }

  return value;
}

mat2 rot(float a) {
  float s = sin(a);
  float c = cos(a);
  return mat2(c, -s, s, c);
}

float kset(vec2 p2) {
  vec3 p = vec3(p2,0.);
  p.xz *= rot(.5);
  p.yz *= rot(.3);
  float c=0.;
  for (float z=1.; z<5.; z++) {
    vec3 pp = vec3(p.xy,z*.03);
    //pp*=.3+z*.3;
    //pp.xy-=time*.05*exp(-z*.5);
    //pp-=.5;
    pp.y-=.7;
    //pp.x-=.2;
    pp.xy*=rot(z*.5-time*(.01+z*.007)*.5);
    //pp*=1.-z*.1;
    //pp+=.5;
    pp.y+=.3;
    pp = abs(.5-fract(pp));
    for (int i=0; i<7; i++) {
      pp = abs(pp)/max(dot(pp,pp),0.003)-.91;
    };
    c+=pow(length(pp),1.3)*.35;
  }
  return c;
}

float ksetaa(vec2 p2) {
  vec2 d = .5/resolution;
  float k0 = kset(p2);
  float k1 = kset(p2 + vec2(d.x, 0.));
  float k2 = kset(p2 + vec2(d.x, d.y));
  float k3 = kset(p2 + vec2(0., d.y));

  return (k0 + k1 + k2 + k3) / 4.0;
}

vec3 star(float i, vec2 p) {
  vec3 c = max(exp(-40.*length(p)),exp(-2000.*abs(p.x*p.y))*exp(-20.*length(p))) * color1;
  c+=exp(-80.*length(p));
  c+=exp(-30.*length(p))*color1*.5;
  c*=1.+hash(vec2(i,floor(time*10.))).x*.3;
  return c;
}

void main() {
  float y = v_texCoord.y;

  vec2 scaledUv = v_texCoord;
  scaledUv = scaledUv - 0.5;
  scaledUv.x *= resolution.x / resolution.y;
  scaledUv /= 1.2;

  vec2 pxUv = v_texCoord;
  pxUv *= resolution;

  vec2 uv = scaledUv;
  uv.x *= textureSize.y / textureSize.x;

  // Make the cat larger and slightly to the right
  // uv.x -= 0.3;
  // uv.y += 0.15;

  vec2 p = uv;
  uv = uv + 0.5;
  vec2 tuv = uv;
  vec4 cat = texture(tex, uv);
  vec2 fuv = fbm((uv-.5)*5.)-.5;
  fuv *= smoothstep(1.,.0,y);
  float sdf = texture(distanceField, tuv+fuv*0.1*exp(-y)).a*smoothstep(1.,0.8,uv.x);
  float f = fbm((uv-.5)*30.).x*0.2*exp(-sdf);
  vec3 col = smoothstep(y*.3+.1,.4,sdf-f) * color1 * .7;
  col = pow(max(0.,sdf*2.2-f*fbmsubstract),falloff) * color1 * .8;
  float k = kset(p+.5);
  sdf = pow(sdf,.4+y*1.5);
  col=clamp(col,0.,1.);
  col+=k*.3*smoothstep(0.,.7,sdf)*color1+pow(k*.2,2.5)*pow(sdf,2.)*2.;
  col*=smoothstep(.93,.8,y);
  col*=smoothstep(.0,.3,y);
  col+=star(1.,(p+vec2(.25,.35))*1.3);
  col+=star(2.,(p+vec2(.2,.27))*2.5);
  col+=star(3.,(p+vec2(.27,.25))*2.);
  col+=star(4.,(p+vec2(.1,.23))*1.8);
  col+=star(5.,(p+vec2(.0,.3))*4.);
  col+=star(6.,(p+vec2(-.15,.3))*3.);
  // col+=star(7.,(p+vec2(-.25,.33))*3.);
  col+=star(8.,(p+vec2(.3,.0))*2.);
  col+=star(9.,(p+vec2(.25,-.13))*3.5);
  // col+=star(10.,(p+vec2(-.25,-.1))*3.5);
  // col+=star(11.,(p+vec2(-.27,-.3))*3.5);
  // col+=star(12.,(p+vec2(-.3,-.05))*5.);
  // col+=star(13.,(p+vec2(-.27,.1))*4.);

  float lum = dot(col, vec3(0.2126, 0.7152, 0.0722));
  fragColor = vec4(col, lum);
  fragColor = mix(fragColor, cat, cat.a);

  fragColor.a *= smoothstep(.0, .1, (resolution.y - pxUv.y) / 400.0);
}

#version 450
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 o;
layout(set = 0, binding = 0) uniform sampler2D tex;
layout(set = 0, binding = 1) uniform Params { float exposure; float gamma; vec2 texel; } p;
vec3 aces(vec3 x) { const float a=2.51, b=0.03, c=2.43, d=0.59, e=0.14; return clamp((x*(a*x+b))/(x*(c*x+d)+e), 0.0, 1.0); }
void main() {
  vec3 c = texture(tex, uv).rgb * p.exposure;
  vec3 blur = vec3(0.0);
  for (int i = -2; i <= 2; ++i) blur += texture(tex, uv + vec2(float(i)) * p.texel).rgb;
  c = mix(c, blur * 0.2, 0.25);
  c = aces(c);
  if (dot(c, vec3(0.2126, 0.7152, 0.0722)) < 0.001) discard;
  o = vec4(pow(c, vec3(1.0 / p.gamma)), 1.0);
}

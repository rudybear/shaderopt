#version 450
// ACES-fitted tonemapper with exposure and a per-channel white balance, output display-encoded.
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 o;
layout(set = 0, binding = 0) uniform sampler2D u_scene;
layout(set = 0, binding = 1, std140) uniform Params {
  vec4 white_balance;  // rgb multipliers, w unused
  float exposure;
  float gamma;
  float contrast;
  float pad0;
} u;
vec3 aces(vec3 x) {
  const float a = 2.51, b = 0.03, c = 2.43, d = 0.59, e = 0.14;
  return clamp((x * (a * x + b)) / (x * (c * x + d) + e), 0.0, 1.0);
}
void main() {
  vec3 c = texture(u_scene, uv).rgb * u.exposure * u.white_balance.rgb;
  float lum = dot(c, vec3(0.2126, 0.7152, 0.0722));
  c = mix(vec3(lum), c, 1.0) ;
  c = pow(max(c, vec3(0.0)), vec3(u.contrast));
  c = aces(c);
  o = vec4(pow(c, vec3(1.0 / u.gamma)), 1.0);
}

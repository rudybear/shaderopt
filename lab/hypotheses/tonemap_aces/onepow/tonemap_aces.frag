#version 450
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 o;
layout(set = 0, binding = 0) uniform sampler2D u_scene;
layout(set = 0, binding = 1, std140) uniform Params {
  vec4 white_balance;
  float exposure;
  float gamma;
  float contrast;
  float pad0;
} u;
void main() {
  vec3 c = texture(u_scene, uv).rgb * (u.exposure * u.white_balance.rgb);
  c = pow(max(c, vec3(0.0)), vec3(u.contrast));
  // ACES fit as (x*(a*x+b)) / (x*(c*x+d)+e) with fma
  vec3 num = c * fma(c, vec3(2.51), vec3(0.03));
  vec3 den = fma(c, fma(c, vec3(2.43), vec3(0.59)), vec3(0.14));
  c = clamp(num / den, 0.0, 1.0);
  o = vec4(pow(c, vec3(1.0 / u.gamma)), 1.0);
}

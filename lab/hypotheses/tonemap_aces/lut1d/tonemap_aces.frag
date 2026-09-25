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
layout(set = 0, binding = 2) uniform sampler2D u_lut;   // 256x1, R = f(x) for x = 2^(mix(-10, 8, t)); provided by the lab from the CPU model
void main() {
  vec3 c = texture(u_scene, uv).rgb * u.exposure * u.white_balance.rgb;
  vec3 t = (log2(max(c, vec3(1e-4))) + 10.0) / 18.0;
  vec3 r = vec3(texture(u_lut, vec2(t.r, 0.5)).r, texture(u_lut, vec2(t.g, 0.5)).r, texture(u_lut, vec2(t.b, 0.5)).r);
  o = vec4(r, 1.0);
}

#version 450
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 o;
layout(set = 0, binding = 0) uniform sampler2D u_src;
layout(set = 0, binding = 1, std140) uniform Params {
  vec4 lift;
  vec4 gamma;
  vec4 gain;
  vec4 mat_r;
  vec4 mat_g;
  vec4 mat_b;
  float saturation;
  float temperature;
  vec2 pad0;
  vec4 h_mat_r;     // mat rows premultiplied by warm (CPU)
  vec4 h_mat_g;
  vec4 h_mat_b;
  vec4 h_inv_gamma; // 1 / max(gamma, 1e-3) (CPU)
} u;
void main() {
  vec3 c = texture(u_src, uv).rgb;
  vec3 m = vec3(dot(u.h_mat_r.rgb, c), dot(u.h_mat_g.rgb, c), dot(u.h_mat_b.rgb, c));
  float lum = dot(m, vec3(0.2126, 0.7152, 0.0722));
  m = mix(vec3(lum), m, u.saturation);
  vec3 g = pow(max(m * u.gain.rgb + u.lift.rgb * (1.0 - m), vec3(0.0)), u.h_inv_gamma.rgb);
  o = vec4(clamp(g, 0.0, 1.0), 1.0);
}

#version 450
// Lift/gamma/gain, saturation and a 3x3 color matrix. Everything but the texture read is uniform-rate math.
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 o;
layout(set = 0, binding = 0) uniform sampler2D u_src;
layout(set = 0, binding = 1, std140) uniform Params {
  vec4 lift;       // rgb, w unused
  vec4 gamma;
  vec4 gain;
  vec4 mat_r;      // rows of a 3x3 matrix
  vec4 mat_g;
  vec4 mat_b;
  float saturation;
  float temperature;   // -1..1
  vec2 pad0;
} u;
void main() {
  vec3 c = texture(u_src, uv).rgb;
  // temperature as a simple blue/orange bias built from uniforms (premultiply candidate)
  vec3 warm = vec3(1.0 + 0.1 * u.temperature, 1.0, 1.0 - 0.1 * u.temperature);
  c *= warm;
  vec3 m = vec3(dot(u.mat_r.rgb, c), dot(u.mat_g.rgb, c), dot(u.mat_b.rgb, c));
  float lum = dot(m, vec3(0.2126, 0.7152, 0.0722));
  m = mix(vec3(lum), m, u.saturation);
  vec3 g = pow(max(m * u.gain.rgb + u.lift.rgb * (1.0 - m), vec3(0.0)), 1.0 / max(u.gamma.rgb, vec3(1e-3)));
  o = vec4(clamp(g, 0.0, 1.0), 1.0);
}

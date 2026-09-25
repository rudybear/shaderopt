#version 450
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 o;
layout(set = 0, binding = 0) uniform sampler2D u_src;
layout(set = 0, binding = 1, std140) uniform Params {
  vec2 texel;
  float sigma;
  float pad0;
} u;
void main() {
  float s2 = u.sigma * u.sigma;
  float w0 = 1.0;
  float w1 = exp(-0.5 * 1.0 / s2), w2 = exp(-0.5 * 4.0 / s2), w3 = exp(-0.5 * 9.0 / s2), w4 = exp(-0.5 * 16.0 / s2);
  float wsum = w0 + 2.0 * (w1 + w2 + w3 + w4);
  // pair taps (1,2) and (3,4): one bilinear fetch each side at the weight-averaged offset
  float wa = w1 + w2, oa = (1.0 * w1 + 2.0 * w2) / wa;
  float wb = w3 + w4, ob = (3.0 * w3 + 4.0 * w4) / wb;
  vec3 acc = texture(u_src, uv).rgb * w0;
  acc += (texture(u_src, uv + vec2(0.0, oa * u.texel.y)).rgb + texture(u_src, uv - vec2(0.0, oa * u.texel.y)).rgb) * wa;
  acc += (texture(u_src, uv + vec2(0.0, ob * u.texel.y)).rgb + texture(u_src, uv - vec2(0.0, ob * u.texel.y)).rgb) * wb;
  o = vec4(acc / wsum, 1.0);
}

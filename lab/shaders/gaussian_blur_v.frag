#version 450
// Separable 9-tap Gaussian, vertical. Weights computed in-shader from sigma (a hoisting candidate).
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 o;
layout(set = 0, binding = 0) uniform sampler2D u_src;
layout(set = 0, binding = 1, std140) uniform Params {
  vec2 texel;      // 1 / source resolution
  float sigma;
  float pad0;
} u;
void main() {
  vec3 acc = vec3(0.0);
  float wsum = 0.0;
  for (int i = -4; i <= 4; ++i) {
    float x = float(i);
    float w = exp(-0.5 * x * x / (u.sigma * u.sigma));
    acc += texture(u_src, uv + vec2(0.0, x * u.texel.y)).rgb * w;
    wsum += w;
  }
  o = vec4(acc / wsum, 1.0);
}

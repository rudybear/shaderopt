#version 450
// Selftest: copy modulated by a uniform block. Proves std140 fill by name and the descriptor-set patch.
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 outColor;
layout(set = 0, binding = 0) uniform sampler2D u_src;
layout(set = 0, binding = 1, std140) uniform Params {
  float gain;
  int channel_swap; // 1: swap R and G
  vec4 tint;
  mat4 xform;       // applied to the color as a vec4
} u;
void main() {
  vec4 c = texture(u_src, uv) * u.gain * u.tint;
  if (u.channel_swap == 1) {
    c = c.grba;
  }
  outColor = u.xform * c;
}

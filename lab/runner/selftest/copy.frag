#version 450
// Selftest: pure copy. Proves uv orientation, sampling at pixel centers and readback.
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 outColor;
layout(set = 0, binding = 0) uniform sampler2D u_src;
void main() {
  outColor = texture(u_src, uv);
}

#version 450
// Vignette plus animated film grain. Cheap; exercises smoothstep, hash noise and the low end of the tolerance gate.
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 o;
layout(set = 0, binding = 0) uniform sampler2D u_src;
layout(set = 0, binding = 1, std140) uniform Params {
  vec2 center;
  float radius;
  float softness;
  float grain_amount;
  float time;
  float aspect;
  float pad0;
} u;
float hash(vec2 p) {
  vec3 p3 = fract(vec3(p.xyx) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.x + p3.y) * p3.z);
}
void main() {
  vec3 c = texture(u_src, uv).rgb;
  vec2 d = (uv - u.center) * vec2(u.aspect, 1.0);
  float v = 1.0 - smoothstep(u.radius, u.radius + u.softness, length(d));
  float g = hash(uv * 1024.0 + vec2(u.time)) - 0.5;
  c = c * v + g * u.grain_amount;
  o = vec4(clamp(c, 0.0, 1.0), 1.0);
}

#version 450
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
  p = fract(p * vec2(0.1031, 0.1030));
  p += dot(p, p.yx + 33.33);
  return fract(p.x * p.y * 95.4337);
}
void main() {
  vec3 c = texture(u_src, uv).rgb;
  vec2 d = (uv - u.center) * vec2(u.aspect, 1.0);
  float d2 = dot(d, d);
  float r0 = u.radius, r1 = u.radius + u.softness;
  float v = 1.0 - smoothstep(r0 * r0, r1 * r1, d2);
  float g = hash(uv * 1024.0 + vec2(u.time)) - 0.5;
  c = c * v + g * u.grain_amount;
  o = vec4(clamp(c, 0.0, 1.0), 1.0);
}

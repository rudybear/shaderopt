#version 450
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 o;
layout(set = 0, binding = 0) uniform sampler2D u_albedo;
layout(set = 0, binding = 1) uniform sampler2D u_normal;
layout(set = 0, binding = 2) uniform sampler2D u_shadowuv;
layout(set = 0, binding = 3) uniform sampler2D u_shadow;
layout(set = 0, binding = 4, std140) uniform Params {
  vec4 light1;
  vec4 light2;
  vec4 ambient;
  vec2 shadow_texel;
  float depth_bias;
  float shadow_min;
} u;
float pcf4(vec3 uvw) {
  vec2 st = uvw.xy / u.shadow_texel - 0.5;
  vec2 f = fract(st);
  vec2 base = (floor(st) + 0.5) * u.shadow_texel;
  float d00 = texture(u_shadow, base).r;
  float d10 = texture(u_shadow, base + vec2(u.shadow_texel.x, 0.0)).r;
  float d01 = texture(u_shadow, base + vec2(0.0, u.shadow_texel.y)).r;
  float d11 = texture(u_shadow, base + u.shadow_texel).r;
  float z = uvw.z + u.depth_bias;
  vec4 lit = vec4(z <= d00 ? 1.0 : 0.0, z <= d10 ? 1.0 : 0.0, z <= d01 ? 1.0 : 0.0, z <= d11 ? 1.0 : 0.0);
  return mix(mix(lit.x, lit.y, f.x), mix(lit.z, lit.w, f.x), f.y);
}
void main() {
  vec4 alb = texture(u_albedo, uv);
  vec4 nrm = texture(u_normal, uv);
  if (alb.a < 0.5 || nrm.w < 0.5) discard;
  vec3 n = normalize(nrm.xyz * 2.0 - 1.0);
  float ndl1 = clamp(dot(n, normalize(u.light1.xyz)), 0.0, 1.0) * u.light1.w;
  float ndl2 = clamp(dot(n, normalize(u.light2.xyz)), 0.0, 1.0) * u.light2.w;
  vec3 suv = texture(u_shadowuv, uv).xyz;
  float sh = 1.0;
  if (suv.z > 0.0 && suv.z < 1.0) sh = mix(u.shadow_min, 1.0, pcf4(suv));
  vec3 c = alb.rgb * (u.ambient.rgb + vec3(0.5 * (ndl1 + ndl2)) * sh);
  o = vec4(c, 1.0);
}

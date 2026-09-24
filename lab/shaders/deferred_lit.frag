#version 450
// Fullscreen deferred lighting adapted from IGL's Tiny_MeshLarge forward shader:
// two hemispherical lights, ambient, and a 3x3 PCF shadow lookup. Inputs are G-buffer textures.
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 o;
layout(set = 0, binding = 0) uniform sampler2D u_albedo;   // rgb albedo, a = 1 where geometry exists
layout(set = 0, binding = 1) uniform sampler2D u_normal;   // xyz world normal in [0,1], w = alpha-test value
layout(set = 0, binding = 2) uniform sampler2D u_shadowuv; // xy shadow-map uv, z shadow-space depth
layout(set = 0, binding = 3) uniform sampler2D u_shadow;   // r = shadow depth
layout(set = 0, binding = 4, std140) uniform Params {
  vec4 light1;      // xyz direction (unnormalized), w intensity
  vec4 light2;
  vec4 ambient;     // rgb
  vec2 shadow_texel;
  float depth_bias;
  float shadow_min;  // 0.3
} u;
float pcf3(vec3 uvw) {
  float s = 0.0;
  for (int v = -1; v <= 1; v++)
    for (int h = -1; h <= 1; h++) {
      float d = texture(u_shadow, uvw.xy + u.shadow_texel * vec2(h, v)).r;
      s += (uvw.z + u.depth_bias <= d) ? 1.0 : 0.0;
    }
  return s / 9.0;
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
  if (suv.z > 0.0 && suv.z < 1.0) sh = mix(u.shadow_min, 1.0, pcf3(suv));
  vec3 c = alb.rgb * (u.ambient.rgb + vec3(0.5 * (ndl1 + ndl2)) * sh);
  o = vec4(c, 1.0);
}

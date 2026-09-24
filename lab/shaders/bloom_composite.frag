#version 450
// Bloom pass 4: upsample the blurred bright pass and add to the scene, then output display-encoded LDR.
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 o;
layout(set = 0, binding = 0) uniform sampler2D u_scene;
layout(set = 0, binding = 1) uniform sampler2D u_bloom;
layout(set = 0, binding = 2, std140) uniform Params {
  float intensity;
  float exposure;
  vec2 pad0;
} u;
vec3 tonemap(vec3 x) { return x / (1.0 + x); }
void main() {
  vec3 scene = texture(u_scene, uv).rgb * u.exposure;
  vec3 bloom = texture(u_bloom, uv).rgb;
  vec3 c = tonemap(scene + bloom * u.intensity);
  o = vec4(pow(c, vec3(1.0 / 2.2)), 1.0);
}

#version 450
// Bloom pass 1: soft-knee luminance threshold. Output is HDR (RGBA16F), usually at half resolution.
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 o;
layout(set = 0, binding = 0) uniform sampler2D u_scene;
layout(set = 0, binding = 1, std140) uniform Params {
  float threshold;   // luminance where bloom starts
  float knee;        // soft knee width
  float exposure;    // pre-exposure multiplier
  float pad0;
} u;
void main() {
  vec3 c = texture(u_scene, uv).rgb * u.exposure;
  float lum = dot(c, vec3(0.2126, 0.7152, 0.0722));
  float k = u.knee * u.threshold;
  float soft = clamp(lum - u.threshold + k, 0.0, 2.0 * k);
  soft = soft * soft / (4.0 * k + 1e-5);
  float contrib = max(soft, lum - u.threshold) / max(lum, 1e-5);
  o = vec4(c * contrib, 1.0);
}

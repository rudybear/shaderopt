#version 450
// FXAA 3.11-style "console" variant, simplified. Branchy; UVs are hard sinks.
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 o;
layout(set = 0, binding = 0) uniform sampler2D u_src;
layout(set = 0, binding = 1, std140) uniform Params {
  vec2 texel;
  float edge_threshold;      // 0.166
  float edge_threshold_min;  // 0.0833
  float subpix;              // 0.75
  float pad0;
  vec2 pad1;
} u;
float luma(vec3 c) { return dot(c, vec3(0.299, 0.587, 0.114)); }
void main() {
  vec3 rgbM = texture(u_src, uv).rgb;
  float lM = luma(rgbM);
  float lN = luma(texture(u_src, uv + vec2(0.0, -u.texel.y)).rgb);
  float lS = luma(texture(u_src, uv + vec2(0.0,  u.texel.y)).rgb);
  float lW = luma(texture(u_src, uv + vec2(-u.texel.x, 0.0)).rgb);
  float lE = luma(texture(u_src, uv + vec2( u.texel.x, 0.0)).rgb);
  float lMin = min(lM, min(min(lN, lS), min(lW, lE)));
  float lMax = max(lM, max(max(lN, lS), max(lW, lE)));
  float range = lMax - lMin;
  if (range < max(u.edge_threshold_min, lMax * u.edge_threshold)) { o = vec4(rgbM, 1.0); return; }
  float lNW = luma(texture(u_src, uv + vec2(-u.texel.x, -u.texel.y)).rgb);
  float lNE = luma(texture(u_src, uv + vec2( u.texel.x, -u.texel.y)).rgb);
  float lSW = luma(texture(u_src, uv + vec2(-u.texel.x,  u.texel.y)).rgb);
  float lSE = luma(texture(u_src, uv + vec2( u.texel.x,  u.texel.y)).rgb);
  float lumaL = 0.25 * (lN + lS + lW + lE);
  float rangeL = abs(lumaL - lM);
  float blendL = clamp((rangeL / range - 0.25) * 4.0, 0.0, 1.0);
  blendL = blendL * blendL * u.subpix;
  vec2 dir;
  dir.x = -((lNW + lNE) - (lSW + lSE));
  dir.y =  ((lNW + lSW) - (lNE + lSE));
  float dirReduce = max((lNW + lNE + lSW + lSE) * 0.03125, 0.0078125);
  float rcpDirMin = 1.0 / (min(abs(dir.x), abs(dir.y)) + dirReduce);
  dir = clamp(dir * rcpDirMin, vec2(-8.0), vec2(8.0)) * u.texel;
  vec3 rgbA = 0.5 * (texture(u_src, uv + dir * (1.0 / 3.0 - 0.5)).rgb + texture(u_src, uv + dir * (2.0 / 3.0 - 0.5)).rgb);
  vec3 rgbB = rgbA * 0.5 + 0.25 * (texture(u_src, uv + dir * -0.5).rgb + texture(u_src, uv + dir * 0.5).rgb);
  float lB = luma(rgbB);
  vec3 result = (lB < lMin || lB > lMax) ? rgbA : rgbB;
  o = vec4(mix(rgbM, result, blendL), 1.0);
}

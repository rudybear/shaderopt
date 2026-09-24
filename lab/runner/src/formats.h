// Contract texture formats -> IGL formats, and decoding of raw texel bytes to float32 RGBA.
#pragma once

#include <cstdint>
#include <string>

#include <igl/Texture.h>

namespace shaderlab {

struct FormatInfo {
  const char* name; // contract spelling
  igl::TextureFormat format;
  uint32_t bytesPerPixel;
};

// Throws std::runtime_error naming the format when it is not one of the contract formats.
const FormatInfo& lookupFormat(const std::string& name);

// Decodes `pixels` texels of `fmt` from `src` into linear float32 RGBA at `dst` (4 floats/texel).
void decodeToFloatRGBA(const FormatInfo& fmt, const uint8_t* src, size_t pixels, float* dst);

float halfToFloat(uint16_t h);
float srgbToLinear(float c);

} // namespace shaderlab

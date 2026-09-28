// Contract texture formats -> IGL formats, decoding of raw texel bytes to float32 RGBA (readback)
// and the mirror encoding of float32 RGBA to texel bytes (input upload).
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

// Encodes `pixels` linear float32 RGBA texels from `src` into `fmt` bytes at `dst`
// (fmt.bytesPerPixel per texel). The exact mirror of decodeToFloatRGBA and of the CPU model's
// formats.quantize: UNORM formats clamp to [0,1], round to nearest even and turn NaN into 0;
// RGBA8_SRGB applies the sRGB OETF to rgb before the 8-bit rounding (the hardware decodes it back on
// sampling); half formats round to nearest even and overflow to Inf; single-channel formats keep .r
// only (sampling returns (r, 0, 0, 1)).
void encodeFromFloatRGBA(const FormatInfo& fmt, const float* src, size_t pixels, uint8_t* dst);

float halfToFloat(uint16_t h);
uint16_t floatToHalf(float f);
float srgbToLinear(float c);
float linearToSrgb(float c);

} // namespace shaderlab

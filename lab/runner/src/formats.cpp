#include "formats.h"

#include <cmath>
#include <cstring>
#include <stdexcept>

namespace shaderlab {

namespace {

// Contract name -> IGL enum. RGB10A2 maps to IGL RGB10_A2_UNorm_Rev = VK_FORMAT_A2R10G10B10_UNORM_PACK32
// (R in bits 20..29, G 10..19, B 0..9, A 30..31). R11G11B10F maps to IGL B10G11R11_UFloat =
// VK_FORMAT_B10G11R11_UFLOAT_PACK32 (R bits 0..10, G 11..21, B 22..31).
const FormatInfo kFormats[] = {
    {"RGBA8", igl::TextureFormat::RGBA_UNorm8, 4},
    {"RGBA8_SRGB", igl::TextureFormat::RGBA_SRGB, 4},
    {"RGBA16F", igl::TextureFormat::RGBA_F16, 8},
    {"RGBA32F", igl::TextureFormat::RGBA_F32, 16},
    {"R11G11B10F", igl::TextureFormat::B10G11R11_UFloat, 4},
    {"RGB10A2", igl::TextureFormat::RGB10_A2_UNorm_Rev, 4},
    {"R16F", igl::TextureFormat::R_F16, 2},
    {"R32F", igl::TextureFormat::R_F32, 4},
};

float unpackUFloat(uint32_t v, uint32_t mantBits) {
  const uint32_t e = (v >> mantBits) & 0x1f;
  const uint32_t m = v & ((1u << mantBits) - 1);
  const float mant = static_cast<float>(m) / static_cast<float>(1u << mantBits);
  if (e == 0) {
    return m == 0 ? 0.0f : std::ldexp(mant, -14);
  }
  if (e == 31) {
    return m == 0 ? INFINITY : NAN;
  }
  return std::ldexp(1.0f + mant, static_cast<int>(e) - 15);
}

} // namespace

float halfToFloat(uint16_t h) {
  const uint32_t sign = (h >> 15) & 1;
  const uint32_t exp = (h >> 10) & 0x1f;
  const uint32_t mant = h & 0x3ff;
  uint32_t bits = 0;
  if (exp == 0) {
    if (mant == 0) {
      bits = sign << 31;
    } else {
      // subnormal half -> normalized float
      uint32_t e = 127 - 15 + 1;
      uint32_t m = mant;
      while ((m & 0x400) == 0) {
        m <<= 1;
        --e;
      }
      m &= 0x3ff;
      bits = (sign << 31) | (e << 23) | (m << 13);
    }
  } else if (exp == 31) {
    bits = (sign << 31) | 0x7f800000u | (mant << 13);
  } else {
    bits = (sign << 31) | ((exp + 127 - 15) << 23) | (mant << 13);
  }
  float f = 0;
  std::memcpy(&f, &bits, 4);
  return f;
}

float srgbToLinear(float c) {
  return c <= 0.04045f ? c / 12.92f : std::pow((c + 0.055f) / 1.055f, 2.4f);
}

const FormatInfo& lookupFormat(const std::string& name) {
  for (const auto& f : kFormats) {
    if (name == f.name) {
      return f;
    }
  }
  std::string known;
  for (const auto& f : kFormats) {
    known += std::string(known.empty() ? "" : ", ") + f.name;
  }
  throw std::runtime_error("unsupported texture format '" + name + "' (accepted: " + known + ")");
}

void decodeToFloatRGBA(const FormatInfo& fmt, const uint8_t* src, size_t pixels, float* dst) {
  const std::string name = fmt.name;
  for (size_t i = 0; i < pixels; ++i) {
    float* o = dst + i * 4;
    const uint8_t* s = src + i * fmt.bytesPerPixel;
    if (name == "RGBA32F") {
      std::memcpy(o, s, 16);
    } else if (name == "RGBA16F") {
      uint16_t h[4];
      std::memcpy(h, s, 8);
      for (int c = 0; c < 4; ++c) {
        o[c] = halfToFloat(h[c]);
      }
    } else if (name == "RGBA8") {
      for (int c = 0; c < 4; ++c) {
        o[c] = s[c] / 255.0f;
      }
    } else if (name == "RGBA8_SRGB") {
      for (int c = 0; c < 3; ++c) {
        o[c] = srgbToLinear(s[c] / 255.0f);
      }
      o[3] = s[3] / 255.0f;
    } else if (name == "R11G11B10F") {
      uint32_t v = 0;
      std::memcpy(&v, s, 4);
      o[0] = unpackUFloat(v & 0x7ff, 6);
      o[1] = unpackUFloat((v >> 11) & 0x7ff, 6);
      o[2] = unpackUFloat((v >> 22) & 0x3ff, 5);
      o[3] = 1.0f;
    } else if (name == "RGB10A2") {
      uint32_t v = 0;
      std::memcpy(&v, s, 4);
      o[0] = ((v >> 20) & 0x3ff) / 1023.0f;
      o[1] = ((v >> 10) & 0x3ff) / 1023.0f;
      o[2] = (v & 0x3ff) / 1023.0f;
      o[3] = ((v >> 30) & 0x3) / 3.0f;
    } else if (name == "R16F") {
      uint16_t h = 0;
      std::memcpy(&h, s, 2);
      o[0] = halfToFloat(h);
      o[1] = o[2] = 0.0f;
      o[3] = 1.0f;
    } else if (name == "R32F") {
      std::memcpy(&o[0], s, 4);
      o[1] = o[2] = 0.0f;
      o[3] = 1.0f;
    } else {
      throw std::runtime_error("decodeToFloatRGBA: no decoder for " + name);
    }
  }
}

} // namespace shaderlab

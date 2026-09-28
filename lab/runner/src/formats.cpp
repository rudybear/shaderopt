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

// float -> unsigned float with a 5-bit exponent and `mantBits` mantissa bits (R11G11B10F channels),
// round to nearest even. Negative and NaN -> 0 (what the CPU model's clip does), overflow -> Inf.
uint32_t packUFloat(float f, uint32_t mantBits) {
  if (!(f > 0.0f)) {
    return 0; // negative, -0, NaN
  }
  if (std::isinf(f)) {
    return 31u << mantBits;
  }
  // Directly on the f32 bits: the 5-bit exponent has the same bias (15) and range as half.
  uint32_t bits = 0;
  std::memcpy(&bits, &f, 4);
  const int32_t e = static_cast<int32_t>((bits >> 23) & 0xff) - 127 + 15; // biased in 5-bit space
  uint32_t m = bits & 0x7fffff; // 23 fraction bits
  const uint32_t drop = 23 - mantBits;
  if (e >= 31) {
    return 31u << mantBits; // overflow -> Inf
  }
  if (e <= 0) {
    // subnormal in the target: value = m' * 2^(-14 - mantBits); shift the implicit 1 in
    if (e < -static_cast<int32_t>(mantBits)) {
      return 0;
    }
    m |= 0x800000;
    const uint32_t shift = drop + static_cast<uint32_t>(1 - e);
    const uint32_t q = m >> shift;
    const uint32_t rem = m & ((1u << shift) - 1);
    const uint32_t half = 1u << (shift - 1);
    uint32_t r = q;
    if (rem > half || (rem == half && (q & 1))) {
      ++r;
    }
    return r; // may carry into the exponent field, which is the right encoding
  }
  const uint32_t q = m >> drop;
  const uint32_t rem = m & ((1u << drop) - 1);
  const uint32_t half = 1u << (drop - 1);
  uint32_t r = (static_cast<uint32_t>(e) << mantBits) | q;
  if (rem > half || (rem == half && (q & 1))) {
    ++r; // carries into the exponent on mantissa overflow; may reach Inf, which is correct
  }
  return r;
}

// UNORM encode: NaN -> 0, clamp to [0,1], round to nearest even (numpy's rounding).
uint32_t toUnorm(float f, uint32_t maxCode) {
  if (std::isnan(f)) {
    return 0;
  }
  const float c = f < 0.0f ? 0.0f : (f > 1.0f ? 1.0f : f);
  return static_cast<uint32_t>(std::nearbyint(c * static_cast<float>(maxCode)));
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

uint16_t floatToHalf(float f) {
  uint32_t bits = 0;
  std::memcpy(&bits, &f, 4);
  const uint32_t sign = (bits >> 16) & 0x8000;
  const uint32_t exp = (bits >> 23) & 0xff;
  uint32_t mant = bits & 0x7fffff;
  if (exp == 0xff) {
    return static_cast<uint16_t>(sign | 0x7c00 | (mant ? (0x200 | (mant >> 13)) : 0)); // Inf / quiet NaN
  }
  const int32_t e = static_cast<int32_t>(exp) - 127 + 15;
  if (e >= 31) {
    return static_cast<uint16_t>(sign | 0x7c00); // overflow -> Inf (numpy float32 -> float16 does the same)
  }
  if (e <= 0) {
    if (e < -10) {
      return static_cast<uint16_t>(sign); // underflows to zero
    }
    mant |= 0x800000;
    const uint32_t shift = static_cast<uint32_t>(14 - e); // 13 + (1 - e)
    const uint32_t q = mant >> shift;
    const uint32_t rem = mant & ((1u << shift) - 1);
    const uint32_t half = 1u << (shift - 1);
    uint32_t r = q;
    if (rem > half || (rem == half && (q & 1))) {
      ++r;
    }
    return static_cast<uint16_t>(sign | r);
  }
  const uint32_t q = mant >> 13;
  const uint32_t rem = mant & 0x1fff;
  uint32_t r = (static_cast<uint32_t>(e) << 10) | q;
  if (rem > 0x1000 || (rem == 0x1000 && (q & 1))) {
    ++r; // mantissa carry rolls into the exponent, possibly to Inf: correct RNE behaviour
  }
  return static_cast<uint16_t>(sign | r);
}

float srgbToLinear(float c) {
  return c <= 0.04045f ? c / 12.92f : std::pow((c + 0.055f) / 1.055f, 2.4f);
}

// sRGB OETF on a value already clamped to [0,1] (the CPU model's formats.srgb_encode).
float linearToSrgb(float c) {
  return c <= 0.0031308f ? c * 12.92f : 1.055f * std::pow(c, 1.0f / 2.4f) - 0.055f;
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

void encodeFromFloatRGBA(const FormatInfo& fmt, const float* src, size_t pixels, uint8_t* dst) {
  const std::string name = fmt.name;
  for (size_t i = 0; i < pixels; ++i) {
    const float* s = src + i * 4;
    uint8_t* o = dst + i * fmt.bytesPerPixel;
    if (name == "RGBA32F") {
      std::memcpy(o, s, 16);
    } else if (name == "RGBA16F") {
      uint16_t h[4];
      for (int c = 0; c < 4; ++c) {
        h[c] = floatToHalf(s[c]);
      }
      std::memcpy(o, h, 8);
    } else if (name == "RGBA8") {
      for (int c = 0; c < 4; ++c) {
        o[c] = static_cast<uint8_t>(toUnorm(s[c], 255));
      }
    } else if (name == "RGBA8_SRGB") {
      for (int c = 0; c < 3; ++c) {
        const float v = std::isnan(s[c]) ? 0.0f : (s[c] < 0.0f ? 0.0f : (s[c] > 1.0f ? 1.0f : s[c]));
        o[c] = static_cast<uint8_t>(toUnorm(linearToSrgb(v), 255));
      }
      o[3] = static_cast<uint8_t>(toUnorm(s[3], 255));
    } else if (name == "R11G11B10F") {
      const uint32_t v = packUFloat(s[0], 6) | (packUFloat(s[1], 6) << 11) | (packUFloat(s[2], 5) << 22);
      std::memcpy(o, &v, 4);
    } else if (name == "RGB10A2") {
      const uint32_t v = (toUnorm(s[0], 1023) << 20) | (toUnorm(s[1], 1023) << 10) | toUnorm(s[2], 1023) |
                         (toUnorm(s[3], 3) << 30);
      std::memcpy(o, &v, 4);
    } else if (name == "R16F") {
      const uint16_t h = floatToHalf(s[0]);
      std::memcpy(o, &h, 2);
    } else if (name == "R32F") {
      std::memcpy(o, &s[0], 4);
    } else {
      throw std::runtime_error("encodeFromFloatRGBA: no encoder for " + name);
    }
  }
}

} // namespace shaderlab

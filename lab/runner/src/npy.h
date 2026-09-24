// Minimal .npy reader/writer for float32 HxWx4 RGBA images (lab/CONTRACTS.md "Images").
#pragma once

#include <cstdint>
#include <string>
#include <vector>

namespace shaderlab {

struct NpyImage {
  uint32_t width = 0;
  uint32_t height = 0;
  std::vector<float> data; // height * width * 4 floats, row 0 = top
};

// Throws std::runtime_error on any format problem. Only '<f4', C order, shape (H, W, 4) accepted.
NpyImage npyRead(const std::string& path);
void npyWrite(const std::string& path, const NpyImage& img);

} // namespace shaderlab

#include "npy.h"

#include <cstdio>
#include <cstring>
#include <fstream>
#include <regex>
#include <stdexcept>

namespace shaderlab {

namespace {

std::vector<uint8_t> readFile(const std::string& path) {
  std::ifstream f(path, std::ios::binary);
  if (!f) {
    throw std::runtime_error("cannot open " + path);
  }
  return std::vector<uint8_t>((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
}

} // namespace

NpyImage npyRead(const std::string& path) {
  const std::vector<uint8_t> bytes = readFile(path);
  if (bytes.size() < 12 || std::memcmp(bytes.data(), "\x93NUMPY", 6) != 0) {
    throw std::runtime_error(path + ": not a .npy file");
  }
  const uint8_t major = bytes[6];
  size_t headerLen = 0;
  size_t headerStart = 0;
  if (major == 1) {
    headerLen = bytes[8] | (bytes[9] << 8);
    headerStart = 10;
  } else if (major == 2 || major == 3) {
    headerLen = bytes[8] | (bytes[9] << 8) | (bytes[10] << 16) | (size_t(bytes[11]) << 24);
    headerStart = 12;
  } else {
    throw std::runtime_error(path + ": unsupported .npy version " + std::to_string(major));
  }
  if (headerStart + headerLen > bytes.size()) {
    throw std::runtime_error(path + ": truncated .npy header");
  }
  const std::string header(reinterpret_cast<const char*>(bytes.data() + headerStart), headerLen);

  const std::regex descrRe("'descr'\\s*:\\s*'([^']*)'");
  const std::regex fortranRe("'fortran_order'\\s*:\\s*(True|False)");
  const std::regex shapeRe("'shape'\\s*:\\s*\\(([^)]*)\\)");
  std::smatch m;
  if (!std::regex_search(header, m, descrRe)) {
    throw std::runtime_error(path + ": .npy header has no descr");
  }
  const std::string descr = m[1];
  if (descr != "<f4") {
    throw std::runtime_error(path + ": .npy dtype must be '<f4' (float32 little-endian), got '" +
                             descr + "'");
  }
  if (!std::regex_search(header, m, fortranRe) || m[1] != "False") {
    throw std::runtime_error(path + ": .npy must be C order (fortran_order False)");
  }
  if (!std::regex_search(header, m, shapeRe)) {
    throw std::runtime_error(path + ": .npy header has no shape");
  }
  std::vector<size_t> shape;
  {
    const std::string s = m[1];
    std::regex numRe("\\d+");
    for (auto it = std::sregex_iterator(s.begin(), s.end(), numRe); it != std::sregex_iterator();
         ++it) {
      shape.push_back(std::stoul(it->str()));
    }
  }
  if (shape.size() != 3 || shape[2] != 4) {
    throw std::runtime_error(path + ": .npy shape must be (H, W, 4)");
  }
  NpyImage img;
  img.height = static_cast<uint32_t>(shape[0]);
  img.width = static_cast<uint32_t>(shape[1]);
  const size_t count = size_t(img.height) * img.width * 4;
  const size_t dataStart = headerStart + headerLen;
  if (dataStart + count * 4 > bytes.size()) {
    throw std::runtime_error(path + ": .npy data truncated");
  }
  img.data.resize(count);
  std::memcpy(img.data.data(), bytes.data() + dataStart, count * 4);
  return img;
}

void npyWrite(const std::string& path, const NpyImage& img) {
  std::string dict = "{'descr': '<f4', 'fortran_order': False, 'shape': (" +
                     std::to_string(img.height) + ", " + std::to_string(img.width) + ", 4), }";
  // total header (magic 6 + ver 2 + len 2 + dict + '\n') must be a multiple of 64
  size_t total = 10 + dict.size() + 1;
  const size_t pad = (64 - (total % 64)) % 64;
  dict.append(pad, ' ');
  dict.push_back('\n');
  std::ofstream f(path, std::ios::binary);
  if (!f) {
    throw std::runtime_error("cannot write " + path);
  }
  const uint16_t len = static_cast<uint16_t>(dict.size());
  f.write("\x93NUMPY\x01\x00", 8);
  f.put(static_cast<char>(len & 0xff));
  f.put(static_cast<char>(len >> 8));
  f.write(dict.data(), static_cast<std::streamsize>(dict.size()));
  f.write(reinterpret_cast<const char*>(img.data.data()),
          static_cast<std::streamsize>(img.data.size() * sizeof(float)));
  if (!f) {
    throw std::runtime_error("write failed: " + path);
  }
}

} // namespace shaderlab

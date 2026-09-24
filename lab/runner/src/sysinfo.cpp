#include "sysinfo.h"

#include <cstdio>
#include <cstdlib>
#include <sys/utsname.h>
#include <sys/stat.h>

namespace shaderlab {

std::string osString() {
  utsname u{};
  if (uname(&u) != 0) {
    return "unknown";
  }
  return std::string(u.sysname) + " " + u.release;
}

namespace {

std::string trim(std::string s) {
  const size_t a = s.find_first_not_of(" \t\r\n");
  const size_t b = s.find_last_not_of(" \t\r\n");
  return a == std::string::npos ? "" : s.substr(a, b - a + 1);
}

bool haveNvidiaSmi() {
  const char* path = std::getenv("PATH");
  if (!path) {
    return false;
  }
  std::string p(path);
  size_t start = 0;
  while (start <= p.size()) {
    size_t end = p.find(':', start);
    if (end == std::string::npos) {
      end = p.size();
    }
    const std::string dir = p.substr(start, end - start);
    struct stat st{};
    if (!dir.empty() && stat((dir + "/nvidia-smi").c_str(), &st) == 0) {
      return true;
    }
    start = end + 1;
  }
  return false;
}

} // namespace

nlohmann::json gpuState() {
  nlohmann::json st;
  st["thermal"] = "unknown";
  st["clocks_mhz"] = {{"gpu", nullptr}, {"mem", nullptr}};
  st["power_state"] = "unknown";
  st["locked_clocks"] = nullptr; // not detectable through nvidia-smi queries; null = unknown
  st["temperature_c"] = nullptr;
  st["throttle_reasons"] = "unknown";
  st["source"] = "none";
  if (!haveNvidiaSmi()) {
    return st;
  }
  FILE* f = popen(
      "nvidia-smi --query-gpu=clocks.gr,clocks.mem,pstate,temperature.gpu,"
      "clocks_throttle_reasons.active --format=csv,noheader 2>/dev/null",
      "r");
  if (!f) {
    return st;
  }
  char line[512] = {};
  const bool got = fgets(line, sizeof(line), f) != nullptr;
  const int rc = pclose(f);
  if (!got || rc != 0) {
    return st;
  }
  // "570 MHz, 810 MHz, P5, 35, 0x0000000000000000"
  std::vector<std::string> fields;
  std::string cur;
  for (const char* c = line; *c; ++c) {
    if (*c == ',') {
      fields.push_back(trim(cur));
      cur.clear();
    } else {
      cur.push_back(*c);
    }
  }
  fields.push_back(trim(cur));
  if (fields.size() < 5) {
    return st;
  }
  st["source"] = "nvidia-smi";
  const auto mhz = [](const std::string& s) -> nlohmann::json {
    try {
      return std::stoi(s);
    } catch (...) {
      return nullptr;
    }
  };
  st["clocks_mhz"] = {{"gpu", mhz(fields[0])}, {"mem", mhz(fields[1])}};
  st["power_state"] = fields[2];
  st["temperature_c"] = mhz(fields[3]);
  st["throttle_reasons"] = fields[4];
  try {
    const unsigned long long mask = std::stoull(fields[4], nullptr, 16);
    // nvmlClocksThrottleReasons: SwThermalSlowdown 0x20, HwThermalSlowdown 0x40
    const bool thermal = (mask & 0x60ull) != 0;
    st["thermal"] = thermal ? "throttled" : "ok";
  } catch (...) {
  }
  return st;
}

} // namespace shaderlab

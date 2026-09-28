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

nlohmann::json emptyState() {
  nlohmann::json st;
  st["thermal"] = "unknown";
  st["clocks_mhz"] = {{"gpu", nullptr}, {"mem", nullptr}};
  st["power_state"] = "unknown";
  st["locked_clocks"] = nullptr; // never asserted by the runner; null = unknown
  st["temperature_c"] = nullptr;
  st["throttle_reasons"] = "unknown";
  st["source"] = "none";
  return st;
}

// Whole stdout of a shell command (bounded), or empty on failure.
std::string runCommand(const char* cmd, size_t maxBytes = 1 << 16) {
  FILE* f = popen(cmd, "r");
  if (!f) {
    return "";
  }
  std::string out;
  char buf[4096];
  size_t n = 0;
  while ((n = fread(buf, 1, sizeof(buf), f)) > 0 && out.size() < maxBytes) {
    out.append(buf, n);
  }
  const int rc = pclose(f);
  return rc == 0 ? out : "";
}

std::vector<std::string> lines(const std::string& s) {
  std::vector<std::string> v;
  size_t start = 0;
  while (start < s.size()) {
    size_t end = s.find('\n', start);
    if (end == std::string::npos) {
      end = s.size();
    }
    v.push_back(s.substr(start, end - start));
    start = end + 1;
  }
  return v;
}

nlohmann::json toNumber(const std::string& s) {
  try {
    size_t used = 0;
    const double d = std::stod(s, &used);
    if (used == 0) {
      return nullptr;
    }
    if (d == static_cast<long long>(d)) {
      return static_cast<long long>(d);
    }
    return d;
  } catch (...) {
    return nullptr;
  }
}

} // namespace

#if defined(__ANDROID__)

#include <filesystem>
#include <fstream>

namespace {

// Android ThermalStatus (android.os.PowerManager.THERMAL_STATUS_*)
const char* thermalStatusName(int s) {
  switch (s) {
  case 0:
    return "none";
  case 1:
    return "light";
  case 2:
    return "moderate";
  case 3:
    return "severe";
  case 4:
    return "critical";
  case 5:
    return "emergency";
  case 6:
    return "shutdown";
  default:
    return "unknown";
  }
}

// android.os.Temperature.TYPE_*
const char* temperatureTypeName(int t) {
  switch (t) {
  case 0:
    return "cpu";
  case 1:
    return "gpu";
  case 2:
    return "battery";
  case 3:
    return "skin";
  case 4:
    return "usb_port";
  case 5:
    return "power_amplifier";
  case 6:
    return "bcl_voltage";
  case 7:
    return "bcl_current";
  case 8:
    return "bcl_percentage";
  case 9:
    return "npu";
  case 10:
    return "tpu";
  case 11:
    return "display";
  case 12:
    return "modem";
  case 13:
    return "soc";
  default:
    return "unknown";
  }
}

// "Temperature{mValue=28.7, mType=0, mName=cpu0-silver-usr, mStatus=0}" -> fields
std::map<std::string, std::string> parseBraceRecord(const std::string& line) {
  std::map<std::string, std::string> out;
  const size_t a = line.find('{');
  const size_t b = line.rfind('}');
  if (a == std::string::npos || b == std::string::npos || b <= a) {
    return out;
  }
  const std::string body = line.substr(a + 1, b - a - 1);
  size_t start = 0;
  while (start < body.size()) {
    size_t end = body.find(',', start);
    if (end == std::string::npos) {
      end = body.size();
    }
    const std::string kv = trim(body.substr(start, end - start));
    const size_t eq = kv.find('=');
    if (eq != std::string::npos) {
      out[trim(kv.substr(0, eq))] = trim(kv.substr(eq + 1));
    }
    start = end + 1;
  }
  return out;
}

// dumpsys thermalservice (Android 10+, runnable by the shell user):
//   Thermal Status: 0
//   Current temperatures from HAL:
//       Temperature{mValue=28.7, mType=0, mName=cpu0-silver-usr, mStatus=0}
void fillThermal(nlohmann::json& st) {
  st["thermal_status"] = nullptr;
  st["temperatures"] = nlohmann::json::object();
  const std::string out = runCommand("dumpsys thermalservice 2>/dev/null");
  if (out.empty()) {
    return;
  }
  st["source"] = "android";
  int status = -1;
  std::string section;
  nlohmann::json temps = nlohmann::json::object();
  nlohmann::json cached = nlohmann::json::object();
  std::vector<std::string> hot;
  nlohmann::json gpuTemp = nullptr;
  nlohmann::json skinTemp = nullptr;
  for (const std::string& raw : lines(out)) {
    const std::string l = trim(raw);
    if (l.rfind("Thermal Status:", 0) == 0) {
      status = std::atoi(trim(l.substr(15)).c_str());
      continue;
    }
    if (l.rfind("Current temperatures", 0) == 0) {
      section = "current";
      continue;
    }
    if (l.rfind("Cached temperatures", 0) == 0) {
      section = "cached";
      continue;
    }
    if (l.rfind("Current cooling devices", 0) == 0 || l.rfind("Temperature static thresholds", 0) == 0) {
      section = "";
      continue;
    }
    if (l.rfind("Temperature{", 0) == 0 && !section.empty()) {
      const auto f = parseBraceRecord(l);
      const auto name = f.count("mName") ? f.at("mName") : "";
      if (name.empty()) {
        continue;
      }
      nlohmann::json rec;
      rec["value"] = toNumber(f.count("mValue") ? f.at("mValue") : "");
      const int type = f.count("mType") ? std::atoi(f.at("mType").c_str()) : -1;
      const int tstatus = f.count("mStatus") ? std::atoi(f.at("mStatus").c_str()) : 0;
      rec["type"] = temperatureTypeName(type);
      rec["status"] = tstatus;
      (section == "current" ? temps : cached)[name] = rec;
      if (section == "current") {
        if (tstatus > 0) {
          hot.push_back(name + "=" + thermalStatusName(tstatus));
        }
        if (type == 1 && gpuTemp.is_null()) {
          gpuTemp = rec["value"];
        }
        if (type == 3 && skinTemp.is_null()) {
          skinTemp = rec["value"];
        }
      }
    }
  }
  if (temps.empty()) {
    temps = cached; // older dumps only list the cached readings
  }
  st["temperatures"] = temps;
  if (status >= 0) {
    st["thermal_status"] = status;
    st["thermal"] = thermalStatusName(status);
  }
  st["temperature_c"] = !gpuTemp.is_null() ? gpuTemp : skinTemp;
  if (status >= 0) {
    std::string reasons;
    for (const auto& h : hot) {
      reasons += (reasons.empty() ? "" : ",") + h;
    }
    st["throttle_reasons"] = reasons.empty() ? "none" : reasons;
  }
}

// dumpsys battery:  "  level: 87", "  status: 2", "  AC powered: false", "  USB powered: true",
// "  temperature: 280" (tenths of a degree C). status: 1 unknown, 2 charging, 3 discharging,
// 4 not charging, 5 full.
void fillBattery(nlohmann::json& st) {
  nlohmann::json b;
  b["level"] = nullptr;
  b["status"] = "unknown";
  b["plugged"] = nullptr;
  b["temperature_c"] = nullptr;
  const std::string out = runCommand("dumpsys battery 2>/dev/null");
  if (!out.empty()) {
    st["source"] = "android";
    bool ac = false, usb = false, wireless = false, plugKnown = false;
    for (const std::string& raw : lines(out)) {
      const std::string l = trim(raw);
      const size_t c = l.find(':');
      if (c == std::string::npos) {
        continue;
      }
      const std::string k = trim(l.substr(0, c));
      const std::string v = trim(l.substr(c + 1));
      if (k == "level") {
        b["level"] = toNumber(v);
      } else if (k == "status") {
        static const char* names[] = {"unknown", "unknown", "charging", "discharging", "not_charging", "full"};
        const int s = std::atoi(v.c_str());
        b["status"] = (s >= 0 && s <= 5) ? names[s] : "unknown";
      } else if (k == "AC powered") {
        ac = v == "true"; plugKnown = true;
      } else if (k == "USB powered") {
        usb = v == "true"; plugKnown = true;
      } else if (k == "Wireless powered") {
        wireless = v == "true"; plugKnown = true;
      } else if (k == "temperature") {
        const nlohmann::json t = toNumber(v);
        if (t.is_number()) {
          b["temperature_c"] = t.get<double>() / 10.0;
        }
      }
    }
    if (plugKnown) {
      b["plugged"] = ac ? "ac" : usb ? "usb" : wireless ? "wireless" : "none";
    }
  }
  st["battery"] = b;
  st["power_state"] = b["status"];
}

std::string readSysfs(const std::string& path) {
  std::ifstream f(path);
  if (!f) {
    return "";
  }
  std::string s;
  std::getline(f, s);
  return trim(s);
}

// GPU clock: Adreno (kgsl) or Mali (mali0 / devfreq). Sysfs is often unreadable for the shell user
// (SELinux), in which case the clock stays null. Values are Hz, kHz or MHz depending on the node.
void fillGpuClock(nlohmann::json& st) {
  namespace fs = std::filesystem;
  std::vector<std::string> candidates = {
      "/sys/class/kgsl/kgsl-3d0/gpuclk",
      "/sys/class/kgsl/kgsl-3d0/devfreq/cur_freq",
      "/sys/class/misc/mali0/device/clock",
      "/sys/class/misc/mali0/device/cur_freq",
  };
  std::error_code ec;
  for (const char* base : {"/sys/class/misc/mali0/device/devfreq", "/sys/class/devfreq"}) {
    for (const auto& e : fs::directory_iterator(base, ec)) {
      const std::string n = e.path().filename().string();
      if (base == std::string("/sys/class/devfreq") && n.find("mali") == std::string::npos &&
          n.find("gpu") == std::string::npos && n.find("g3d") == std::string::npos &&
          n.find("kgsl") == std::string::npos) {
        continue;
      }
      candidates.push_back((e.path() / "cur_freq").string());
    }
    ec.clear();
  }
  st["clock_source"] = nullptr;
  for (const auto& p : candidates) {
    const std::string v = readSysfs(p);
    const nlohmann::json n = toNumber(v);
    if (!n.is_number()) {
      continue;
    }
    double hz = n.get<double>();
    if (hz > 1e8) {
      // Hz
    } else if (hz > 1e5) {
      hz *= 1e3; // kHz
    } else {
      hz *= 1e6; // MHz
    }
    st["clocks_mhz"]["gpu"] = static_cast<long long>(hz / 1e6 + 0.5);
    st["clock_source"] = p;
    break;
  }
}

} // namespace

nlohmann::json gpuState() {
  nlohmann::json st = emptyState();
  fillThermal(st);
  fillBattery(st);
  fillGpuClock(st);
  return st;
}

#else // desktop: nvidia-smi

namespace {

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
  nlohmann::json st = emptyState();
  if (!haveNvidiaSmi()) {
    return st;
  }
  const std::string line = runCommand(
      "nvidia-smi --query-gpu=clocks.gr,clocks.mem,pstate,temperature.gpu,"
      "clocks_throttle_reasons.active --format=csv,noheader 2>/dev/null");
  if (line.empty()) {
    return st;
  }
  // "570 MHz, 810 MHz, P5, 35, 0x0000000000000000"
  std::vector<std::string> fields;
  std::string cur;
  for (const char c : line) {
    if (c == '\n') {
      break;
    }
    if (c == ',') {
      fields.push_back(trim(cur));
      cur.clear();
    } else {
      cur.push_back(c);
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

#endif

} // namespace shaderlab

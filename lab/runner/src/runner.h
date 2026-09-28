// GPU execution of a job on IGL's Vulkan backend: chain of fullscreen passes, timestamps, readback.
#pragma once

#include <functional>
#include <map>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

#include "npy.h"
#include "scenario.h"

namespace shaderlab {

struct RunConfig {
  int deviceIndex = -1; // -1: first discrete GPU; otherwise index into the physical device list
  bool validation = true;
};

struct RunResult {
  nlohmann::json device; // result.json "device" object
  std::map<std::string, std::vector<double>> timingsNs; // pass -> one entry per sample
  std::map<std::string, NpyImage> images; // pass -> readback (only when readback == last)
  std::vector<std::string> notes; // informational, goes to stderr and result "notes"
};

// Opens the device like runJob and returns {"device": <result.json device object>, "devices":
// [{index,name,type}], "notes": [...]} without needing a job (`--info`). Throws on failure.
nlohmann::json deviceInfo(const RunConfig& cfg);

// Runs everything. Throws std::runtime_error on any failure. `device` in the result is filled as
// soon as the device exists, so callers can report it even when a later step fails: pass a
// pointer to receive it early.
RunResult runJob(const Job& job,
                 const Scenario& scenario,
                 const RunConfig& cfg,
                 nlohmann::json* deviceOut);

} // namespace shaderlab

// shaderlab-runner: headless IGL/Vulkan executor for lab job bundles (see lab/CONTRACTS.md).
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <string>

#include <nlohmann/json.hpp>

#include "npy.h"
#include "runner.h"
#include "scenario.h"
#include "sysinfo.h"
#include "validation.h"

#ifndef SHADERLAB_IGL_COMMIT
#define SHADERLAB_IGL_COMMIT "unknown"
#endif
#ifndef SHADERLAB_RUNNER_BUILD
#define SHADERLAB_RUNNER_BUILD "unknown"
#endif

namespace fs = std::filesystem;
using nlohmann::json;

namespace {

void usage() {
  fprintf(stderr,
          "usage: shaderlab-runner --job <dir>/job.json --out <result_dir> [--device N] "
          "[--no-validation] [--verbose]\n"
          "       shaderlab-runner --info [--device N] [--no-validation]   (device + state JSON on stdout)\n"
          "  env: SHADERLAB_DEVICE_INDEX=N (same as --device), SHADERLAB_VERBOSE=1\n");
}

json validationJson() {
  const auto s = shaderlab::validation::summary();
  json v;
  v["errors"] = s.errors;
  v["warnings"] = s.warnings;
  json msgs = json::array();
  for (const auto& e : s.messages) {
    msgs.push_back(std::to_string(e.count) + "x " + (e.isError ? "[error] " : "[warning] ") + e.text);
  }
  v["messages"] = msgs;
  return v;
}

} // namespace

int main(int argc, char** argv) {
  std::string jobPath;
  std::string outDir;
  shaderlab::RunConfig cfg;
  bool verbose = std::getenv("SHADERLAB_VERBOSE") != nullptr;
  bool info = false;
  if (const char* e = std::getenv("SHADERLAB_DEVICE_INDEX")) {
    cfg.deviceIndex = std::atoi(e);
  }
  for (int i = 1; i < argc; ++i) {
    const std::string a = argv[i];
    const auto next = [&](const char* flag) -> std::string {
      if (i + 1 >= argc) {
        fprintf(stderr, "%s needs a value\n", flag);
        usage();
        std::exit(1);
      }
      return argv[++i];
    };
    if (a == "--job") {
      jobPath = next("--job");
    } else if (a == "--out") {
      outDir = next("--out");
    } else if (a == "--device") {
      cfg.deviceIndex = std::atoi(next("--device").c_str());
    } else if (a == "--no-validation") {
      cfg.validation = false;
    } else if (a == "--verbose") {
      verbose = true;
    } else if (a == "--info") {
      info = true;
    } else if (a == "-h" || a == "--help") {
      usage();
      return 0;
    } else {
      fprintf(stderr, "unknown argument %s\n", a.c_str());
      usage();
      return 1;
    }
  }
  if (info) {
    // Probe mode: no job, no files written. Used by `lab android devices` to identify the GPU.
    shaderlab::validation::install(verbose);
    json j;
    j["ok"] = false;
    j["tools"] = {{"igl_commit", SHADERLAB_IGL_COMMIT}, {"runner_build", SHADERLAB_RUNNER_BUILD}};
    j["os"] = shaderlab::osString();
    j["state"] = shaderlab::gpuState();
    try {
      const json d = shaderlab::deviceInfo(cfg);
      j["device"] = d["device"];
      j["devices"] = d["devices"];
      j["notes"] = d["notes"];
      j["ok"] = true;
    } catch (const std::exception& e) {
      j["error"] = e.what();
    }
    printf("%s\n", j.dump(2).c_str());
    return j["ok"].get<bool>() ? 0 : 1;
  }
  if (jobPath.empty() || outDir.empty()) {
    usage();
    return 1;
  }

  json result;
  result["schema"] = 1;
  result["variant_id"] = nullptr;
  result["scenario"] = nullptr;
  result["ok"] = false;
  result["error"] = nullptr;
  result["device"] = nullptr;
  result["tools"] = {{"igl_commit", SHADERLAB_IGL_COMMIT}, {"runner_build", SHADERLAB_RUNNER_BUILD}};
  result["state"] = nullptr;
  result["validation"] = {{"errors", 0}, {"warnings", 0}, {"messages", json::array()}};
  result["timings_ns"] = json::object();
  result["images"] = json::object();
  result["notes"] = json::array();

  const auto writeResult = [&]() {
    std::error_code ec;
    fs::create_directories(outDir, ec);
    const std::string path = (fs::path(outDir) / "result.json").string();
    std::ofstream f(path);
    if (!f) {
      fprintf(stderr, "cannot write %s\n", path.c_str());
      return;
    }
    f << result.dump(2) << "\n";
  };

  shaderlab::validation::install(verbose);
  json deviceJson;
  try {
    const shaderlab::Job job = shaderlab::loadJob(jobPath);
    result["variant_id"] = job.variantId;
    result["inflight"] = job.inflight;
    const shaderlab::Scenario scenario = shaderlab::loadScenario(job.scenarioPath);
    result["scenario"] = scenario.name;
    shaderlab::validateJob(job, scenario);
    result["state"] = shaderlab::gpuState(); // sampled before the run

    shaderlab::RunResult rr = shaderlab::runJob(job, scenario, cfg, &deviceJson);
    result["device"] = rr.device;
    for (const auto& n : rr.notes) {
      fprintf(stderr, "%s\n", n.c_str());
      result["notes"].push_back(n);
    }
    for (const auto& [pass, t] : rr.timingsNs) {
      result["timings_ns"][pass] = t;
    }
    {
      json clocks = json::array();
      for (double c : rr.sampleClockMhz) {
        if (std::isnan(c)) { clocks.push_back(nullptr); } else { clocks.push_back(c); }
      }
      result["sample_clock_mhz"] = clocks;
    }
    std::error_code ec;
    fs::create_directories(outDir, ec);
    for (const auto& [pass, img] : rr.images) {
      const std::string file = pass + ".npy";
      shaderlab::npyWrite((fs::path(outDir) / file).string(), img);
      result["images"][pass] = file;
    }
    result["state_after"] = shaderlab::gpuState();
    result["validation"] = validationJson();
    result["ok"] = true;
    result["error"] = nullptr;
    writeResult();
    return 0;
  } catch (const std::exception& e) {
    if (!deviceJson.is_null()) {
      result["device"] = deviceJson;
    }
    result["validation"] = validationJson();
    result["ok"] = false;
    result["error"] = e.what();
    fprintf(stderr, "error: %s\n", e.what());
    writeResult();
    return 1;
  }
}

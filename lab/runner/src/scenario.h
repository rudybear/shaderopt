// Scenario (.toml) and job bundle (job.json) models, per lab/CONTRACTS.md.
#pragma once

#include <cstdint>
#include <map>
#include <string>
#include <vector>

namespace shaderlab {

struct UniformValue {
  enum class Kind { Int, Float, Vec, Mat4 };
  Kind kind = Kind::Float;
  std::vector<double> values; // 1 for scalars, 2..4 for vec, 16 for mat4 (column-major)
  bool allIntegers = false; // every literal was a TOML integer (so it may fill an int member)
  std::string describe() const;
};

struct PassDesc {
  std::string name;
  std::string shader;
  std::map<std::string, std::string> samplers; // sampler name -> input or earlier pass name
  std::map<std::string, UniformValue> uniforms; // uniform block member -> value
  std::string format; // output texture format name (contract spelling)
  double scale = 1.0;
  std::string load = "dont_care"; // dont_care | clear | load
  std::string store = "store"; // store | dont_care
  std::string sampler = "linear"; // linear | nearest
};

struct InputDesc {
  std::string name;
  std::string format = "RGBA32F"; // texture format the input is uploaded in (contract spelling)
};

struct Scenario {
  std::string name;
  std::string split;
  uint32_t width = 0;
  uint32_t height = 0;
  std::vector<InputDesc> inputs; // in declaration order
  std::vector<PassDesc> passes;
  std::vector<std::string> qualityOutputs;
};

struct Job {
  std::string dir; // directory containing job.json; relative paths resolve against it
  int schema = 0;
  std::string scenarioPath; // absolute
  std::string variantId;
  std::map<std::string, std::string> passes; // pass name -> absolute .spv path
  std::map<std::string, std::string> inputs; // input name -> absolute .npy path
  int samples = 0;
  int iterations = 0;
  int warmup = 0;
  std::string readback = "last"; // last | none
};

Scenario loadScenario(const std::string& path);
Job loadJob(const std::string& jobJsonPath);

// Cross-checks the job against the scenario (every pass has a .spv, every input a file, values in
// range). Throws std::runtime_error with a precise message.
void validateJob(const Job& job, const Scenario& scenario);

} // namespace shaderlab

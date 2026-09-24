#include "scenario.h"

#include <filesystem>
#include <fstream>
#include <set>
#include <sstream>
#include <stdexcept>

#include <nlohmann/json.hpp>
#include <toml.hpp>

namespace shaderlab {

namespace fs = std::filesystem;

std::string UniformValue::describe() const {
  switch (kind) {
  case Kind::Int:
    return "int";
  case Kind::Float:
    return "float";
  case Kind::Vec:
    return "vec" + std::to_string(values.size());
  case Kind::Mat4:
    return "mat4";
  }
  return "?";
}

namespace {

[[noreturn]] void fail(const std::string& what) {
  throw std::runtime_error(what);
}

UniformValue parseUniform(const std::string& pass, const std::string& name, const toml::node& n) {
  UniformValue v;
  if (n.is_integer()) {
    v.kind = UniformValue::Kind::Int;
    v.values = {static_cast<double>(n.value<int64_t>().value())};
    v.allIntegers = true;
    return v;
  }
  if (n.is_floating_point()) {
    v.kind = UniformValue::Kind::Float;
    v.values = {n.value<double>().value()};
    return v;
  }
  if (n.is_array()) {
    const auto& arr = *n.as_array();
    v.allIntegers = true;
    for (const auto& e : arr) {
      if (e.is_integer()) {
        v.values.push_back(static_cast<double>(e.value<int64_t>().value()));
      } else if (e.is_floating_point()) {
        v.values.push_back(e.value<double>().value());
        v.allIntegers = false;
      } else {
        fail("scenario pass '" + pass + "': uniform '" + name + "' array must contain numbers");
      }
    }
    if (v.values.size() >= 2 && v.values.size() <= 4) {
      v.kind = UniformValue::Kind::Vec;
    } else if (v.values.size() == 16) {
      v.kind = UniformValue::Kind::Mat4;
    } else {
      fail("scenario pass '" + pass + "': uniform '" + name + "' has " +
           std::to_string(v.values.size()) + " elements; expected 2, 3, 4 (vecN) or 16 (mat4)");
    }
    return v;
  }
  fail("scenario pass '" + pass + "': uniform '" + name +
       "' must be a number or an array of numbers");
}

template<typename T>
T req(const toml::table& t, const char* key, const std::string& ctx) {
  const auto v = t[key].value<T>();
  if (!v) {
    fail(ctx + ": missing or mistyped key '" + std::string(key) + "'");
  }
  return *v;
}

} // namespace

Scenario loadScenario(const std::string& path) {
  toml::table tbl;
  try {
    tbl = toml::parse_file(path);
  } catch (const toml::parse_error& e) {
    std::ostringstream os;
    os << "scenario " << path << ": TOML parse error: " << e.description() << " at line "
       << e.source().begin.line;
    fail(os.str());
  }
  Scenario s;
  const toml::table* sc = tbl["scenario"].as_table();
  if (!sc) {
    fail("scenario " + path + ": missing [scenario] table");
  }
  s.name = req<std::string>(*sc, "name", "[scenario]");
  s.split = (*sc)["split"].value_or(std::string("train"));
  const int64_t w = req<int64_t>(*sc, "width", "[scenario]");
  const int64_t h = req<int64_t>(*sc, "height", "[scenario]");
  if (w <= 0 || h <= 0 || w > 16384 || h > 16384) {
    fail("[scenario]: width/height out of range");
  }
  s.width = static_cast<uint32_t>(w);
  s.height = static_cast<uint32_t>(h);

  if (const toml::array* inputs = tbl["inputs"].as_array()) {
    for (const auto& n : *inputs) {
      const toml::table* it = n.as_table();
      if (!it) {
        fail("[[inputs]] entries must be tables");
      }
      s.inputs.push_back(req<std::string>(*it, "name", "[[inputs]]"));
    }
  }

  const toml::array* passes = tbl["passes"].as_array();
  if (!passes || passes->empty()) {
    fail("scenario " + path + ": needs at least one [[passes]] entry");
  }
  std::set<std::string> seen;
  for (const auto& n : *passes) {
    const toml::table* pt = n.as_table();
    if (!pt) {
      fail("[[passes]] entries must be tables");
    }
    PassDesc p;
    p.name = req<std::string>(*pt, "name", "[[passes]]");
    const std::string ctx = "[[passes]] '" + p.name + "'";
    if (!seen.insert(p.name).second) {
      fail(ctx + ": duplicate pass name");
    }
    p.shader = req<std::string>(*pt, "shader", ctx);
    if (const toml::table* st = (*pt)["samplers"].as_table()) {
      for (const auto& [k, v] : *st) {
        const auto sv = v.value<std::string>();
        if (!sv) {
          fail(ctx + ": samplers." + std::string(k.str()) + " must be a string");
        }
        p.samplers[std::string(k.str())] = *sv;
      }
    }
    if (const toml::table* ut = (*pt)["uniforms"].as_table()) {
      for (const auto& [k, v] : *ut) {
        p.uniforms[std::string(k.str())] = parseUniform(p.name, std::string(k.str()), v);
      }
    }
    const toml::table* ot = (*pt)["output"].as_table();
    if (!ot) {
      fail(ctx + ": missing output = { format = ..., scale = ... }");
    }
    p.format = req<std::string>(*ot, "format", ctx + ".output");
    if (const auto sc2 = (*ot)["scale"].value<double>()) {
      p.scale = *sc2;
    } else if (const auto sci = (*ot)["scale"].value<int64_t>()) {
      p.scale = static_cast<double>(*sci);
    }
    if (!(p.scale > 0.0) || p.scale > 16.0) {
      fail(ctx + ": output.scale must be in (0, 16]");
    }
    p.load = (*pt)["load"].value_or(std::string("dont_care"));
    p.store = (*pt)["store"].value_or(std::string("store"));
    p.sampler = (*pt)["sampler"].value_or(std::string("linear"));
    if (p.load != "dont_care" && p.load != "clear" && p.load != "load") {
      fail(ctx + ": load must be dont_care | clear | load");
    }
    if (p.store != "store" && p.store != "dont_care") {
      fail(ctx + ": store must be store | dont_care");
    }
    if (p.sampler != "linear" && p.sampler != "nearest") {
      fail(ctx + ": sampler must be linear | nearest");
    }
    s.passes.push_back(std::move(p));
  }
  if (const toml::table* q = tbl["quality"].as_table()) {
    if (const toml::array* outs = (*q)["outputs"].as_array()) {
      for (const auto& o : *outs) {
        if (const auto sv = o.value<std::string>()) {
          s.qualityOutputs.push_back(*sv);
        }
      }
    }
  }
  if (s.qualityOutputs.empty()) {
    s.qualityOutputs.push_back(s.passes.back().name);
  }
  return s;
}

Job loadJob(const std::string& jobJsonPath) {
  std::ifstream f(jobJsonPath);
  if (!f) {
    fail("cannot open job file " + jobJsonPath);
  }
  nlohmann::json j;
  try {
    j = nlohmann::json::parse(f);
  } catch (const nlohmann::json::exception& e) {
    fail("job.json parse error: " + std::string(e.what()));
  }
  Job job;
  job.dir = fs::absolute(fs::path(jobJsonPath)).parent_path().string();
  const auto rel = [&](const std::string& p) {
    const fs::path pp(p);
    return pp.is_absolute() ? pp.lexically_normal().string()
                            : (fs::path(job.dir) / pp).lexically_normal().string();
  };
  try {
    job.schema = j.at("schema").get<int>();
    job.scenarioPath = rel(j.at("scenario").get<std::string>());
    job.variantId = j.at("variant_id").get<std::string>();
    for (const auto& [k, v] : j.at("passes").items()) {
      job.passes[k] = rel(v.get<std::string>());
    }
    if (j.contains("inputs")) {
      for (const auto& [k, v] : j.at("inputs").items()) {
        job.inputs[k] = rel(v.get<std::string>());
      }
    }
    job.samples = j.at("samples").get<int>();
    job.iterations = j.at("iterations").get<int>();
    job.warmup = j.at("warmup").get<int>();
    job.readback = j.value("readback", std::string("last"));
  } catch (const nlohmann::json::exception& e) {
    fail("job.json: " + std::string(e.what()));
  }
  if (job.schema != 1) {
    fail("job.json: unsupported schema " + std::to_string(job.schema));
  }
  if (job.samples < 1 || job.iterations < 1 || job.warmup < 0) {
    fail("job.json: samples >= 1, iterations >= 1, warmup >= 0 required");
  }
  if (job.readback != "last" && job.readback != "none") {
    fail("job.json: readback must be 'last' or 'none'");
  }
  return job;
}

void validateJob(const Job& job, const Scenario& scenario) {
  for (const auto& p : scenario.passes) {
    const auto it = job.passes.find(p.name);
    if (it == job.passes.end()) {
      fail("job.json: passes has no entry for scenario pass '" + p.name + "'");
    }
    if (!fs::exists(it->second)) {
      fail("job.json: SPIR-V for pass '" + p.name + "' not found: " + it->second);
    }
  }
  for (const auto& [k, v] : job.passes) {
    bool known = false;
    for (const auto& p : scenario.passes) {
      known = known || p.name == k;
    }
    if (!known) {
      fail("job.json: passes entry '" + k + "' is not a pass of scenario '" + scenario.name + "'");
    }
  }
  for (const auto& name : scenario.inputs) {
    const auto it = job.inputs.find(name);
    if (it == job.inputs.end()) {
      fail("job.json: inputs has no entry for scenario input '" + name + "'");
    }
    if (!fs::exists(it->second)) {
      fail("job.json: input '" + name + "' not found: " + it->second);
    }
  }
  // sampler sources must be an input or an earlier pass
  std::set<std::string> available(scenario.inputs.begin(), scenario.inputs.end());
  for (const auto& p : scenario.passes) {
    for (const auto& [sname, src] : p.samplers) {
      if (!available.count(src)) {
        fail("scenario pass '" + p.name + "': sampler '" + sname + "' refers to '" + src +
             "', which is neither an input nor an earlier pass");
      }
    }
    available.insert(p.name);
  }
}

} // namespace shaderlab

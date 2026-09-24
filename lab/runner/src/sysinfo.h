// Host / GPU state for result.json: uname, nvidia-smi, tool versions.
#pragma once

#include <string>

#include <nlohmann/json.hpp>

namespace shaderlab {

std::string osString(); // "Linux 6.17.0-20-generic"
nlohmann::json gpuState(); // {"thermal","clocks_mhz","power_state","locked_clocks",...}

} // namespace shaderlab

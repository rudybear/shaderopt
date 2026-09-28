// Host / GPU state for result.json: uname; nvidia-smi on desktop; dumpsys thermalservice / dumpsys
// battery / GPU devfreq sysfs on Android.
#pragma once

#include <string>

#include <nlohmann/json.hpp>

namespace shaderlab {

std::string osString(); // "Linux 6.17.0-20-generic"
nlohmann::json gpuState(); // {"thermal","clocks_mhz","power_state","locked_clocks",...}; Android adds
                           // "thermal_status" (0..6), "temperatures", "battery", "clock_source"

} // namespace shaderlab

// Cheap per-sample GPU clock read (MHz), NaN when no readable clock source exists on this host.
double gpuClockMhzNow();

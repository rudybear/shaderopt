// Captures Khronos validation-layer messages that IGL routes through its logger
// (src/igl/vulkan/VulkanContext.cpp vulkanDebugCallback -> IGL_LOG_INFO) via IGLLogSetHandler.
#pragma once

#include <string>
#include <utility>
#include <vector>

namespace shaderlab::validation {

struct Entry {
  std::string text; // first-seen message text (prefix stripped)
  int count = 0;
  bool isError = false;
};

struct Summary {
  int errors = 0; // total error messages (with multiplicity)
  int warnings = 0; // total non-error messages (warnings + performance hints)
  std::vector<Entry> messages; // deduplicated, in first-seen order
};

// Installs the log handler. `verbose` forwards IGL info logs to stderr as well.
void install(bool verbose);
Summary summary();

} // namespace shaderlab::validation

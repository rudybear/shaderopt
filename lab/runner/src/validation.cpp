#include "validation.h"

#include <cstdarg>
#include <cstdio>
#include <map>
#include <mutex>
#include <regex>

#include <igl/Common.h>

namespace shaderlab::validation {

namespace {

std::mutex gMutex;
std::vector<Entry> gEntries;
std::map<std::string, size_t> gIndex; // normalized key -> index in gEntries
int gErrors = 0;
int gWarnings = 0;
bool gVerbose = false;
IGLLogHandlerFunc gPrevious = nullptr;

std::string normalizeKey(const std::string& s) {
  // Handles and pointers differ between otherwise identical messages.
  static const std::regex hexRe("0x[0-9a-fA-F]+");
  return std::regex_replace(s, hexRe, "0x*");
}

void record(std::string msg) {
  // strip leading newlines and the IGL prefix
  size_t p = msg.find_first_not_of("\n\r ");
  if (p != std::string::npos) {
    msg.erase(0, p);
  }
  bool isError = false;
  if (msg.rfind("ERROR:", 0) == 0) {
    isError = true;
    msg.erase(0, 6);
  } else if (msg.rfind("PERFORMANCE:", 0) == 0) {
    msg.erase(0, 12);
  }
  p = msg.find_first_not_of("\n\r ");
  if (p != std::string::npos) {
    msg.erase(0, p);
  }
  if (msg.rfind("Validation layer:", 0) == 0) {
    msg.erase(0, 17);
    p = msg.find_first_not_of("\n\r ");
    if (p != std::string::npos) {
      msg.erase(0, p);
    }
  }
  while (!msg.empty() && (msg.back() == '\n' || msg.back() == ' ')) {
    msg.pop_back();
  }
  const std::string key = normalizeKey(msg);
  std::lock_guard<std::mutex> lock(gMutex);
  (isError ? gErrors : gWarnings)++;
  const auto it = gIndex.find(key);
  if (it == gIndex.end()) {
    gIndex[key] = gEntries.size();
    gEntries.push_back({msg, 1, isError});
  } else {
    gEntries[it->second].count++;
  }
}

int handler(IGLLogLevel level, const char* format, va_list ap) {
  va_list copy;
  va_copy(copy, ap);
  const int needed = vsnprintf(nullptr, 0, format, copy);
  va_end(copy);
  std::string msg;
  if (needed > 0) {
    msg.resize(static_cast<size_t>(needed) + 1);
    va_copy(copy, ap);
    vsnprintf(msg.data(), msg.size(), format, copy);
    va_end(copy);
    msg.resize(static_cast<size_t>(needed));
  }
  const bool isValidation = msg.find("Validation layer:") != std::string::npos;
  if (isValidation) {
    record(msg);
    fprintf(stderr, "[validation] %s\n", msg.c_str());
    return needed;
  }
  if (level != IGLLogInfo || gVerbose) {
    fprintf(stderr, "[igl%s] %s",
            level == IGLLogError ? " error" : (level == IGLLogWarning ? " warning" : ""),
            msg.c_str());
    if (msg.empty() || msg.back() != '\n') {
      fputc('\n', stderr);
    }
  }
  return needed;
}

} // namespace

void install(bool verbose) {
  gVerbose = verbose;
  gPrevious = IGLLogGetHandler();
  IGLLogSetHandler(&handler);
}

Summary summary() {
  std::lock_guard<std::mutex> lock(gMutex);
  return Summary{gErrors, gWarnings, gEntries};
}

} // namespace shaderlab::validation

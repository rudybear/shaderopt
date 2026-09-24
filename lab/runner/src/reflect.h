// SPIR-V interface discovery with SPIRV-Reflect, plus the descriptor-set patch IGL needs.
#pragma once

#include <cstdint>
#include <string>
#include <vector>

namespace shaderlab {

struct SamplerBinding {
  std::string name;
  uint32_t set = 0;
  uint32_t binding = 0;
};

struct UniformMember {
  enum class Type { Float, Int, Vec2, Vec3, Vec4, Mat4, Other };
  std::string name;
  uint32_t offset = 0; // bytes from block start (std140, as reflected)
  uint32_t size = 0;
  Type type = Type::Other;
  std::string typeName; // human readable, for errors
};

struct UniformBlock {
  bool present = false;
  std::string name; // block type name ("Params")
  uint32_t set = 0;
  uint32_t binding = 0;
  uint32_t size = 0; // padded size in bytes
  std::vector<UniformMember> members;
};

struct OutputVar {
  std::string name;
  uint32_t location = 0;
};

struct ShaderInterface {
  std::string entryPoint;
  std::vector<SamplerBinding> samplers;
  UniformBlock block;
  std::vector<OutputVar> outputs;
  std::vector<uint32_t> inputLocations;
  std::vector<std::string> otherDescriptors; // anything else found (reported as an error)
};

std::vector<uint32_t> readSpirv(const std::string& path);

// Throws std::runtime_error with the SPIRV-Reflect failure or an interface violation.
ShaderInterface reflectFragment(const std::vector<uint32_t>& words, const std::string& label);

// Rewrites the DescriptorSet decoration of every OpVariable in the Uniform or StorageBuffer storage
// class to `targetSet`. IGL's Vulkan backend hardwires buffers to descriptor set 1 and combined
// image samplers to set 0. Returns the number of decorations changed.
uint32_t patchBufferDescriptorSets(std::vector<uint32_t>& words, uint32_t targetSet);

} // namespace shaderlab

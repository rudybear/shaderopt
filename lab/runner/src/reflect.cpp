#include "reflect.h"

#include <cstring>
#include <fstream>
#include <set>
#include <stdexcept>

#include <spirv_reflect.h>

namespace shaderlab {

std::vector<uint32_t> readSpirv(const std::string& path) {
  std::ifstream f(path, std::ios::binary);
  if (!f) {
    throw std::runtime_error("cannot open SPIR-V " + path);
  }
  std::vector<char> bytes((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
  if (bytes.size() < 20 || bytes.size() % 4 != 0) {
    throw std::runtime_error(path + ": not a SPIR-V module (size " + std::to_string(bytes.size()) +
                             ")");
  }
  std::vector<uint32_t> words(bytes.size() / 4);
  std::memcpy(words.data(), bytes.data(), bytes.size());
  if (words[0] != 0x07230203u) {
    throw std::runtime_error(path + ": bad SPIR-V magic");
  }
  return words;
}

namespace {

UniformMember::Type classify(const SpvReflectTypeDescription* t, std::string& name) {
  if (!t) {
    name = "<unknown>";
    return UniformMember::Type::Other;
  }
  const uint32_t flags = t->type_flags;
  const bool isFloat = (flags & SPV_REFLECT_TYPE_FLAG_FLOAT) != 0;
  const bool isInt = (flags & SPV_REFLECT_TYPE_FLAG_INT) != 0;
  const bool isVec = (flags & SPV_REFLECT_TYPE_FLAG_VECTOR) != 0;
  const bool isMat = (flags & SPV_REFLECT_TYPE_FLAG_MATRIX) != 0;
  const bool isArr = (flags & SPV_REFLECT_TYPE_FLAG_ARRAY) != 0;
  const bool isStruct = (flags & SPV_REFLECT_TYPE_FLAG_STRUCT) != 0;
  const uint32_t width = t->traits.numeric.scalar.width;
  if (isArr || isStruct || width != 32) {
    name = isArr ? "array" : (isStruct ? "struct" : "non-32-bit scalar");
    return UniformMember::Type::Other;
  }
  if (isMat) {
    const uint32_t c = t->traits.numeric.matrix.column_count;
    const uint32_t r = t->traits.numeric.matrix.row_count;
    name = "mat" + std::to_string(c) + (c == r ? "" : "x" + std::to_string(r));
    return (isFloat && c == 4 && r == 4) ? UniformMember::Type::Mat4 : UniformMember::Type::Other;
  }
  if (isVec) {
    const uint32_t n = t->traits.numeric.vector.component_count;
    name = std::string(isFloat ? "vec" : (isInt ? "ivec" : "?vec")) + std::to_string(n);
    if (!isFloat) {
      return UniformMember::Type::Other;
    }
    switch (n) {
    case 2:
      return UniformMember::Type::Vec2;
    case 3:
      return UniformMember::Type::Vec3;
    case 4:
      return UniformMember::Type::Vec4;
    default:
      return UniformMember::Type::Other;
    }
  }
  if (isFloat) {
    name = "float";
    return UniformMember::Type::Float;
  }
  if (isInt) {
    name = t->traits.numeric.scalar.signedness ? "int" : "uint";
    return UniformMember::Type::Int;
  }
  if (flags & SPV_REFLECT_TYPE_FLAG_BOOL) {
    name = "bool";
    return UniformMember::Type::Other;
  }
  name = "<unsupported>";
  return UniformMember::Type::Other;
}

} // namespace

ShaderInterface reflectFragment(const std::vector<uint32_t>& words, const std::string& label) {
  SpvReflectShaderModule mod{};
  const SpvReflectResult r =
      spvReflectCreateShaderModule(words.size() * sizeof(uint32_t), words.data(), &mod);
  if (r != SPV_REFLECT_RESULT_SUCCESS) {
    throw std::runtime_error(label + ": SPIRV-Reflect failed with code " + std::to_string(r));
  }
  struct Guard {
    SpvReflectShaderModule* m;
    ~Guard() {
      spvReflectDestroyShaderModule(m);
    }
  } guard{&mod};

  ShaderInterface si;
  if (mod.entry_point_count < 1) {
    throw std::runtime_error(label + ": SPIR-V has no entry point");
  }
  if (mod.shader_stage != SPV_REFLECT_SHADER_STAGE_FRAGMENT_BIT) {
    throw std::runtime_error(label + ": SPIR-V is not a fragment shader");
  }
  si.entryPoint = mod.entry_point_name ? mod.entry_point_name : "main";

  uint32_t count = 0;
  spvReflectEnumerateDescriptorBindings(&mod, &count, nullptr);
  std::vector<SpvReflectDescriptorBinding*> bindings(count);
  spvReflectEnumerateDescriptorBindings(&mod, &count, bindings.data());
  for (const SpvReflectDescriptorBinding* b : bindings) {
    if (b->descriptor_type == SPV_REFLECT_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER) {
      if (b->image.dim != SpvDim2D || b->image.arrayed || b->image.ms) {
        si.otherDescriptors.push_back(std::string("sampler '") + (b->name ? b->name : "?") +
                                      "' is not a plain sampler2D");
        continue;
      }
      si.samplers.push_back({b->name ? b->name : "", b->set, b->binding});
    } else if (b->descriptor_type == SPV_REFLECT_DESCRIPTOR_TYPE_UNIFORM_BUFFER) {
      if (si.block.present) {
        si.otherDescriptors.push_back("more than one uniform block");
        continue;
      }
      si.block.present = true;
      si.block.name = (b->type_description && b->type_description->type_name)
                          ? b->type_description->type_name
                          : (b->name ? b->name : "");
      si.block.set = b->set;
      si.block.binding = b->binding;
      si.block.size = b->block.padded_size ? b->block.padded_size : b->block.size;
      for (uint32_t i = 0; i < b->block.member_count; ++i) {
        const SpvReflectBlockVariable& m = b->block.members[i];
        UniformMember um;
        um.name = m.name ? m.name : "";
        um.offset = m.absolute_offset;
        um.size = m.size;
        um.type = classify(m.type_description, um.typeName);
        si.block.members.push_back(std::move(um));
      }
    } else {
      si.otherDescriptors.push_back(std::string("descriptor '") + (b->name ? b->name : "?") +
                                    "' of unsupported type " +
                                    std::to_string(static_cast<int>(b->descriptor_type)) +
                                    " (only sampler2D and one uniform block are allowed)");
    }
  }

  count = 0;
  spvReflectEnumerateOutputVariables(&mod, &count, nullptr);
  std::vector<SpvReflectInterfaceVariable*> outs(count);
  spvReflectEnumerateOutputVariables(&mod, &count, outs.data());
  for (const SpvReflectInterfaceVariable* v : outs) {
    if (v->decoration_flags & SPV_REFLECT_DECORATION_BUILT_IN) {
      continue;
    }
    si.outputs.push_back({v->name ? v->name : "", v->location});
  }
  count = 0;
  spvReflectEnumerateInputVariables(&mod, &count, nullptr);
  std::vector<SpvReflectInterfaceVariable*> ins(count);
  spvReflectEnumerateInputVariables(&mod, &count, ins.data());
  for (const SpvReflectInterfaceVariable* v : ins) {
    if (v->decoration_flags & SPV_REFLECT_DECORATION_BUILT_IN) {
      continue;
    }
    si.inputLocations.push_back(v->location);
  }
  if (mod.push_constant_block_count > 0) {
    si.otherDescriptors.push_back("push constants are not allowed (CONTRACTS.md)");
  }
  return si;
}

uint32_t patchBufferDescriptorSets(std::vector<uint32_t>& words, uint32_t targetSet) {
  constexpr uint32_t kOpVariable = 59;
  constexpr uint32_t kOpDecorate = 71;
  constexpr uint32_t kDecorationDescriptorSet = 34;
  constexpr uint32_t kStorageClassUniform = 2;
  constexpr uint32_t kStorageClassStorageBuffer = 12;

  std::set<uint32_t> bufferVars;
  for (size_t i = 5; i < words.size();) {
    const uint32_t op = words[i] & 0xffffu;
    const uint32_t len = words[i] >> 16;
    if (len == 0 || i + len > words.size()) {
      throw std::runtime_error("malformed SPIR-V instruction stream");
    }
    if (op == kOpVariable && len >= 4) {
      const uint32_t sc = words[i + 3];
      if (sc == kStorageClassUniform || sc == kStorageClassStorageBuffer) {
        bufferVars.insert(words[i + 2]);
      }
    }
    i += len;
  }
  uint32_t changed = 0;
  for (size_t i = 5; i < words.size();) {
    const uint32_t op = words[i] & 0xffffu;
    const uint32_t len = words[i] >> 16;
    if (op == kOpDecorate && len >= 4 && words[i + 2] == kDecorationDescriptorSet &&
        bufferVars.count(words[i + 1]) && words[i + 3] != targetSet) {
      words[i + 3] = targetSet;
      ++changed;
    }
    i += len;
  }
  return changed;
}

} // namespace shaderlab

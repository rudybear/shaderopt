#include "runner.h"

#include <cmath>
#include <cstdio>
#include <cstring>
#include <set>
#include <sstream>
#include <stdexcept>

#include <igl/IGL.h>
#include <igl/TimestampQueries.h>
#include <igl/vulkan/Common.h>
#include <igl/vulkan/Device.h>
#include <igl/vulkan/HWDevice.h>
#include <igl/vulkan/VulkanContext.h>
#include <igl/vulkan/VulkanFeatures.h>

#include "formats.h"
#include "reflect.h"
#include "sysinfo.h"

namespace shaderlab {

namespace {

// The built-in fullscreen triangle (CONTRACTS.md "Shaders"). IGL prepends "#version 460" and its
// extension header when the source has no #version line.
const char* kFullscreenVertexGlsl =
    "layout(location=0) out vec2 uv; void main(){ uv = vec2((gl_VertexIndex << 1) & 2, "
    "gl_VertexIndex & 2); gl_Position = vec4(uv * vec2(2,-2) + vec2(-1,1), 0.0, 1.0); }";

[[noreturn]] void fail(const std::string& what) {
  throw std::runtime_error(what);
}

void check(const igl::Result& r, const std::string& what) {
  if (!r.isOk()) {
    fail(what + ": " + (r.message.empty() ? "IGL error" : r.message));
  }
}

std::string apiVersionString(uint32_t v) {
  return std::to_string(VK_API_VERSION_MAJOR(v)) + "." + std::to_string(VK_API_VERSION_MINOR(v)) +
         "." + std::to_string(VK_API_VERSION_PATCH(v));
}

std::string driverVersionString(const igl::vulkan::VulkanContext& ctx) {
  const auto& drv = ctx.getVkPhysicalDeviceDriverProperties();
  if (drv.driverInfo[0] != '\0') {
    return drv.driverInfo;
  }
  const auto& props = ctx.getVkPhysicalDeviceProperties();
  const uint32_t v = props.driverVersion;
  if (props.vendorID == 0x10DE) { // NVIDIA: 10.8.8.6 bits
    return std::to_string(v >> 22) + "." + std::to_string((v >> 14) & 0xff) + "." +
           std::to_string((v >> 6) & 0xff);
  }
  return apiVersionString(v);
}

// VK_KHR_shader_float_controls2 is newer than the system Vulkan headers (1.3.275), so declare the
// feature struct locally. sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_FLOAT_CONTROLS_2_FEATURES_KHR.
struct PhysicalDeviceShaderFloatControls2Features {
  VkStructureType sType;
  void* pNext;
  VkBool32 shaderFloatControls2;
};
constexpr VkStructureType kStructureTypeFloatControls2 = static_cast<VkStructureType>(1000528000);

nlohmann::json queryDeviceJson(const igl::vulkan::VulkanContext& ctx) {
  const auto& props = ctx.getVkPhysicalDeviceProperties();
  nlohmann::json d;
  d["name"] = props.deviceName;
  d["driver"] = driverVersionString(ctx);
  d["api"] = apiVersionString(props.apiVersion);
  d["vendor_id"] = props.vendorID;
  d["device_id"] = props.deviceID;
  d["timestamp_period_ns"] = props.limits.timestampPeriod;
  d["timestamp_compute_and_graphics"] = props.limits.timestampComputeAndGraphics == VK_TRUE;
  d["os"] = osString();

  // Physical-device supported features, queried directly through IGL's volk function table.
  VkPhysicalDeviceShaderFloat16Int8Features f16{
      .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES};
  VkPhysicalDevice16BitStorageFeatures s16{
      .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_16BIT_STORAGE_FEATURES};
  PhysicalDeviceShaderFloatControls2Features fc2{kStructureTypeFloatControls2, nullptr, VK_FALSE};
  const bool hasFc2Ext = ctx.features().available("VK_KHR_shader_float_controls2",
                                                  igl::vulkan::VulkanFeatures::ExtensionType::Device);
  f16.pNext = &s16;
  s16.pNext = hasFc2Ext ? static_cast<void*>(&fc2) : nullptr;
  VkPhysicalDeviceFeatures2 f2{.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2,
                               .pNext = &f16};
  nlohmann::json features;
  if (ctx.vf_.vkGetPhysicalDeviceFeatures2 && ctx.getVkPhysicalDevice()) {
    ctx.vf_.vkGetPhysicalDeviceFeatures2(ctx.getVkPhysicalDevice(), &f2);
    features["shaderFloat16"] = f16.shaderFloat16 == VK_TRUE;
    features["storageBuffer16BitAccess"] = s16.storageBuffer16BitAccess == VK_TRUE;
    features["uniformAndStorageBuffer16BitAccess"] =
        s16.uniformAndStorageBuffer16BitAccess == VK_TRUE;
    features["storageInputOutput16"] = s16.storageInputOutput16 == VK_TRUE;
    if (hasFc2Ext) {
      features["shaderFloatControls2"] = fc2.shaderFloatControls2 == VK_TRUE;
    } else {
      features["shaderFloatControls2"] = nullptr; // extension not advertised; not queried
    }
  } else {
    features["shaderFloat16"] = nullptr;
    features["storageBuffer16BitAccess"] = nullptr;
    features["shaderFloatControls2"] = nullptr;
  }
  d["features"] = features;
  // What IGL actually enabled on the logical device (VulkanFeatures chain).
  const auto& enabled = ctx.features();
  d["enabled_by_igl"] = {
      {"shaderFloat16", enabled.featuresShaderFloat16Int8.shaderFloat16 == VK_TRUE},
      {"storageBuffer16BitAccess", enabled.features16BitStorage.storageBuffer16BitAccess == VK_TRUE},
      {"shaderFloatControls2", false}, // IGL does not chain this feature struct
  };
  return d;
}

struct PassGpu {
  const PassDesc* desc = nullptr;
  ShaderInterface iface;
  uint32_t width = 0;
  uint32_t height = 0;
  const FormatInfo* format = nullptr;
  std::shared_ptr<igl::ITexture> output;
  std::shared_ptr<igl::IFramebuffer> framebuffer;
  std::shared_ptr<igl::IRenderPipelineState> pipeline;
  std::unique_ptr<igl::IBuffer> ubo;
  std::vector<std::pair<uint32_t, std::shared_ptr<igl::ITexture>>> textures; // binding -> texture
  std::shared_ptr<igl::ISamplerState> sampler;
};

// Members are declared in creation order so destruction runs in reverse (device last).
struct Gpu {
  std::unique_ptr<igl::vulkan::Device> device;
  std::shared_ptr<igl::ICommandQueue> queue;
  std::shared_ptr<igl::IShaderModule> vertexModule;
  std::shared_ptr<igl::ISamplerState> samplerLinear;
  std::shared_ptr<igl::ISamplerState> samplerNearest;
  std::map<std::string, std::shared_ptr<igl::ITexture>> inputs;
  std::vector<PassGpu> passes;
  std::shared_ptr<igl::ITimestampQueries> timestamps;
};

std::vector<uint8_t> buildUniformBuffer(const PassDesc& pass, const UniformBlock& block) {
  std::vector<uint8_t> bytes(std::max<uint32_t>(16, (block.size + 15) & ~15u), 0);
  std::vector<std::string> missing;
  std::vector<std::string> mismatched;
  std::set<std::string> provided;
  for (const auto& [name, val] : pass.uniforms) {
    const UniformMember* m = nullptr;
    for (const auto& mm : block.members) {
      if (mm.name == name) {
        m = &mm;
      }
    }
    if (!m) {
      missing.push_back(name);
      continue;
    }
    provided.insert(name);
    const auto put = [&](const float* f, size_t n) {
      if (m->offset + n * 4 > bytes.size()) {
        fail("pass '" + pass.name + "': uniform '" + name + "' offset out of block range");
      }
      std::memcpy(bytes.data() + m->offset, f, n * 4);
    };
    std::vector<float> f(val.values.begin(), val.values.end());
    bool ok = false;
    switch (m->type) {
    case UniformMember::Type::Float:
      ok = (val.kind == UniformValue::Kind::Float || val.kind == UniformValue::Kind::Int);
      if (ok) {
        put(f.data(), 1);
      }
      break;
    case UniformMember::Type::Int:
      ok = val.kind == UniformValue::Kind::Int;
      if (ok) {
        const int32_t i = static_cast<int32_t>(val.values[0]);
        std::memcpy(bytes.data() + m->offset, &i, 4);
      }
      break;
    case UniformMember::Type::Vec2:
    case UniformMember::Type::Vec3:
    case UniformMember::Type::Vec4: {
      const size_t n = m->type == UniformMember::Type::Vec2 ? 2
                       : m->type == UniformMember::Type::Vec3 ? 3
                                                              : 4;
      ok = val.kind == UniformValue::Kind::Vec && val.values.size() == n;
      if (ok) {
        put(f.data(), n);
      }
      break;
    }
    case UniformMember::Type::Mat4:
      ok = val.kind == UniformValue::Kind::Mat4 && m->size == 64;
      if (ok) {
        put(f.data(), 16); // std140 mat4: four vec4 columns, contiguous
      }
      break;
    case UniformMember::Type::Other:
      ok = false;
      break;
    }
    if (!ok) {
      mismatched.push_back(name + " (scenario " + val.describe() + ", shader " + m->typeName + ")");
    }
  }
  std::vector<std::string> unset;
  for (const auto& mm : block.members) {
    if (!provided.count(mm.name)) {
      // Padding members (name starts with "pad") are zero-filled and never required by the scenario.
      if (mm.name.rfind("pad", 0) == 0) {
        continue;
      }
      unset.push_back(mm.name + ":" + mm.typeName);
    }
  }
  const auto join = [](const std::vector<std::string>& v) {
    std::string s;
    for (const auto& x : v) {
      s += (s.empty() ? "" : ", ") + x;
    }
    return s;
  };
  std::string err;
  if (!missing.empty()) {
    err += " uniforms not in the shader's block '" + block.name + "': " + join(missing) + ".";
  }
  if (!mismatched.empty()) {
    err += " type mismatches: " + join(mismatched) + ".";
  }
  if (!unset.empty()) {
    err += " block members not set by the scenario: " + join(unset) + ".";
  }
  if (!err.empty()) {
    fail("pass '" + pass.name + "':" + err);
  }
  return bytes;
}

igl::LoadAction toLoad(const std::string& s) {
  if (s == "clear") {
    return igl::LoadAction::Clear;
  }
  if (s == "load") {
    return igl::LoadAction::Load;
  }
  return igl::LoadAction::DontCare;
}

igl::StoreAction toStore(const std::string& s) {
  return s == "dont_care" ? igl::StoreAction::DontCare : igl::StoreAction::Store;
}

} // namespace

namespace {

struct OpenedDevice {
  std::unique_ptr<igl::vulkan::Device> device;
  nlohmann::json deviceJson;
  nlohmann::json deviceList = nlohmann::json::array();
  std::vector<std::string> notes;
};

// Context + logical device with no window, no surface, no swapchain (shared by runJob and --info).
// Default device: the first discrete GPU; otherwise the only device when it is an integrated GPU
// (phones, laptops with one iGPU); otherwise an error listing the devices.
OpenedDevice openDevice(const RunConfig& cfg) {
  using namespace igl;
  OpenedDevice out;
  Result res;
  vulkan::VulkanContextConfig ctxCfg;
  ctxCfg.enableValidation = cfg.validation;
  ctxCfg.enableGPUAssistedValidation = false;
  ctxCfg.enableExtraLogs = false;
  ctxCfg.terminateOnValidationError = false;
  ctxCfg.headless = false; // never VK_EXT_headless_surface (driver segfault, see DISCOVERY.md 5)
  ctxCfg.enableDescriptorIndexing = false;
  ctxCfg.applicationName = "shaderlab-runner";
  std::unique_ptr<vulkan::VulkanContext> ctx = vulkan::HWDevice::createContext(ctxCfg, nullptr, nullptr);
  if (!ctx) {
    fail("VulkanContext creation failed");
  }
  const std::vector<HWDeviceDesc> devices =
      vulkan::HWDevice::queryDevices(*ctx, HWDeviceQueryDesc(HWDeviceType::Unknown), &res);
  check(res, "queryDevices");
  if (devices.empty()) {
    fail("no Vulkan physical devices found");
  }
  std::string deviceList;
  for (size_t i = 0; i < devices.size(); ++i) {
    deviceList += "\n  [" + std::to_string(i) + "] " + devices[i].name + " (type " +
                  std::to_string(static_cast<int>(devices[i].type)) + ")";
    out.deviceList.push_back({{"index", i}, {"name", devices[i].name}, {"type", static_cast<int>(devices[i].type)}});
  }
  const HWDeviceDesc* chosen = nullptr;
  if (cfg.deviceIndex >= 0) {
    if (static_cast<size_t>(cfg.deviceIndex) >= devices.size()) {
      fail("device index " + std::to_string(cfg.deviceIndex) + " out of range; devices:" + deviceList);
    }
    chosen = &devices[static_cast<size_t>(cfg.deviceIndex)];
  } else {
    for (const auto& d : devices) {
      if (d.type == HWDeviceType::DiscreteGpu) {
        chosen = &d;
        break;
      }
    }
    if (!chosen && devices.size() == 1 && devices[0].type == HWDeviceType::IntegratedGpu) {
      chosen = &devices[0]; // the only GPU (mobile SoC)
    }
    if (!chosen) {
      fail("no discrete GPU found; pass --device N to pick one of:" + deviceList);
    }
  }
  out.notes.push_back("device: " + chosen->name + " (of" + deviceList + "\n)");
  out.device = vulkan::HWDevice::create(
      std::move(ctx), *chosen, /*width*/ 0, /*height*/ 0, 0, nullptr, nullptr, "shaderlab-runner", &res);
  check(res, "HWDevice::create");
  if (!out.device) {
    fail("HWDevice::create returned null");
  }
  const vulkan::VulkanContext& vctx = out.device->getVulkanContext();
  out.deviceJson = queryDeviceJson(vctx);
  out.deviceJson["validation_layer_active"] = vctx.areValidationLayersEnabled();
  if (cfg.validation && !vctx.areValidationLayersEnabled()) {
    out.notes.push_back("WARNING: validation requested but VK_LAYER_KHRONOS_validation is not active");
  }
  return out;
}

} // namespace

nlohmann::json deviceInfo(const RunConfig& cfg) {
  OpenedDevice od = openDevice(cfg);
  nlohmann::json j;
  j["device"] = od.deviceJson;
  j["devices"] = od.deviceList;
  j["notes"] = od.notes;
  return j;
}

RunResult runJob(const Job& job,
                 const Scenario& scenario,
                 const RunConfig& cfg,
                 nlohmann::json* deviceOut) {
  using namespace igl;
  RunResult out;
  Gpu gpu;
  Result res;

  // ---- context and device (no window, no surface, no swapchain) ----
  {
    OpenedDevice od = openDevice(cfg);
    gpu.device = std::move(od.device);
    out.device = std::move(od.deviceJson);
    out.notes = std::move(od.notes);
  }
  if (deviceOut) {
    *deviceOut = out.device;
  }
  const vulkan::VulkanContext& vctx = gpu.device->getVulkanContext();
  if (vctx.getVkPhysicalDeviceProperties().limits.timestampComputeAndGraphics != VK_TRUE) {
    fail("device does not support timestampComputeAndGraphics");
  }

  gpu.queue = gpu.device->createCommandQueue(CommandQueueDesc{}, &res);
  check(res, "createCommandQueue");

  // ---- built-in vertex stage ----
  {
    ShaderModuleInfo info;
    info.stage = ShaderStage::Vertex;
    info.entryPoint = "main";
    gpu.vertexModule = gpu.device->createShaderModule(
        ShaderModuleDesc::fromStringInput(kFullscreenVertexGlsl, info, "fullscreen_triangle_vs"),
        &res);
    check(res, "compile built-in fullscreen vertex shader");
  }

  // ---- samplers ----
  {
    SamplerStateDesc sd;
    sd.minFilter = sd.magFilter = SamplerMinMagFilter::Linear;
    sd.mipFilter = SamplerMipFilter::Disabled;
    sd.addressModeU = sd.addressModeV = sd.addressModeW = SamplerAddressMode::Clamp;
    sd.debugName = "linear_clamp";
    gpu.samplerLinear = gpu.device->createSamplerState(sd, &res);
    check(res, "createSamplerState(linear)");
    sd.minFilter = sd.magFilter = SamplerMinMagFilter::Nearest;
    sd.debugName = "nearest_clamp";
    gpu.samplerNearest = gpu.device->createSamplerState(sd, &res);
    check(res, "createSamplerState(nearest)");
  }

  // ---- inputs: .npy -> RGBA32F sampled textures ----
  for (const auto& name : scenario.inputs) {
    const NpyImage img = npyRead(job.inputs.at(name));
    TextureDesc td = TextureDesc::new2D(TextureFormat::RGBA_F32, img.width, img.height,
                                        TextureDesc::TextureUsageBits::Sampled, name.c_str());
    auto tex = gpu.device->createTexture(td, &res);
    check(res, "createTexture(input " + name + ")");
    const Result up = tex->upload(TextureRangeDesc::new2D(0, 0, img.width, img.height),
                                  img.data.data(), img.width * 16);
    check(up, "upload(input " + name + ")");
    gpu.inputs[name] = std::move(tex);
  }

  // ---- passes ----
  std::map<std::string, std::shared_ptr<ITexture>> produced = gpu.inputs;
  for (const PassDesc& pd : scenario.passes) {
    PassGpu pg;
    pg.desc = &pd;
    const std::string spvPath = job.passes.at(pd.name);
    std::vector<uint32_t> words = readSpirv(spvPath);
    pg.iface = reflectFragment(words, "pass '" + pd.name + "' (" + spvPath + ")");
    const ShaderInterface& si = pg.iface;
    const std::string ctxs = "pass '" + pd.name + "'";
    if (!si.otherDescriptors.empty()) {
      std::string s;
      for (const auto& x : si.otherDescriptors) {
        s += "\n  " + x;
      }
      fail(ctxs + ": unsupported shader interface:" + s);
    }
    if (si.outputs.empty()) {
      fail(ctxs + ": fragment shader has no location-qualified output");
    }
    for (const auto& o : si.outputs) {
      if (o.location != 0) {
        fail(ctxs + ": output '" + o.name + "' at location " + std::to_string(o.location) +
             "; the runner supports exactly one color output at location 0");
      }
    }
    if (si.outputs.size() != 1) {
      fail(ctxs + ": more than one fragment output at location 0");
    }
    for (uint32_t loc : si.inputLocations) {
      if (loc != 0) {
        fail(ctxs + ": fragment input at location " + std::to_string(loc) +
             "; the built-in vertex stage only provides uv at location 0");
      }
    }
    {
      std::vector<std::string> nameless;
      for (const auto& s : si.samplers) {
        if (s.name.empty()) {
          nameless.push_back("sampler at binding " + std::to_string(s.binding));
        }
      }
      if (si.block.present) {
        for (const auto& m : si.block.members) {
          if (m.name.empty()) {
            nameless.push_back("uniform block member at offset " + std::to_string(m.offset));
          }
        }
      }
      if (!nameless.empty()) {
        std::string s;
        for (const auto& x : nameless) {
          s += (s.empty() ? "" : "; ") + x;
        }
        fail(ctxs + ": the SPIR-V carries no OpName debug names (" + s +
             "). Binding is by name, so the module must keep names: compile with 'glslang -V' "
             "(without -g0, which strips OpName) or do not run spirv-opt --strip-debug");
      }
    }
    for (const auto& s : si.samplers) {
      if (s.set != 0) {
        fail(ctxs + ": sampler '" + s.name + "' is in descriptor set " + std::to_string(s.set) +
             "; samplers must use set 0 (CONTRACTS.md; IGL binds textures in set 0)");
      }
      if (s.binding >= IGL_TEXTURE_SAMPLERS_MAX) {
        fail(ctxs + ": sampler '" + s.name + "' binding " + std::to_string(s.binding) +
             " exceeds IGL's limit of " + std::to_string(IGL_TEXTURE_SAMPLERS_MAX));
      }
    }
    if (si.block.present && si.block.binding >= IGL_UNIFORM_BLOCKS_BINDING_MAX) {
      fail(ctxs + ": uniform block binding " + std::to_string(si.block.binding) +
           " exceeds IGL's limit of " + std::to_string(IGL_UNIFORM_BLOCKS_BINDING_MAX));
    }
    if (!si.block.present && !pd.uniforms.empty()) {
      std::string names;
      for (const auto& [k, v] : pd.uniforms) {
        names += (names.empty() ? "" : ", ") + k;
      }
      fail(ctxs + ": scenario sets uniforms (" + names + ") but the shader has no uniform block");
    }

    // sampler name -> texture
    for (const auto& s : si.samplers) {
      const auto it = pd.samplers.find(s.name);
      if (it == pd.samplers.end()) {
        fail(ctxs + ": shader sampler '" + s.name + "' has no entry in the scenario's samplers table");
      }
      const auto tex = produced.find(it->second);
      if (tex == produced.end()) {
        fail(ctxs + ": sampler '" + s.name + "' -> '" + it->second + "' is not available yet");
      }
      pg.textures.emplace_back(s.binding, tex->second);
    }
    for (const auto& [sname, src] : pd.samplers) {
      bool used = false;
      for (const auto& s : si.samplers) {
        used = used || s.name == sname;
      }
      if (!used) {
        out.notes.push_back("note: " + ctxs + " maps sampler '" + sname + "' which the shader does not declare");
      }
    }

    // uniform block -> std140 bytes (IGL wants buffers in descriptor set 1: patch the decoration)
    if (si.block.present) {
      const std::vector<uint8_t> bytes = buildUniformBuffer(pd, si.block);
      BufferDesc bd;
      bd.type = BufferDesc::BufferTypeBits::Uniform;
      bd.data = bytes.data();
      bd.length = bytes.size();
      bd.debugName = pd.name + ".ubo";
      pg.ubo = gpu.device->createBuffer(bd, &res);
      check(res, ctxs + ": createBuffer(ubo)");
      const uint32_t patched = patchBufferDescriptorSets(words, vulkan::kBindPoint_Buffers);
      if (patched) {
        out.notes.push_back("note: " + ctxs + ": rewrote " + std::to_string(patched) +
                            " DescriptorSet decoration(s) of buffer variables to set " +
                            std::to_string(vulkan::kBindPoint_Buffers) + " for IGL");
      }
    }

    // fragment module + pipeline
    pg.format = &lookupFormat(pd.format);
    {
      const auto caps = gpu.device->getTextureFormatCapabilities(pg.format->format);
      const auto need = ICapabilities::TextureFormatCapabilityBits::Attachment |
                        ICapabilities::TextureFormatCapabilityBits::Sampled;
      if ((caps & need) != need) {
        fail(ctxs + ": format " + pd.format + " is not usable as a sampled color attachment on this device/IGL (caps=" +
             std::to_string(static_cast<int>(caps)) + ")");
      }
    }
    ShaderModuleInfo finfo;
    finfo.stage = ShaderStage::Fragment;
    finfo.entryPoint = si.entryPoint;
    auto fragModule = gpu.device->createShaderModule(
        ShaderModuleDesc::fromBinaryInput(words.data(), words.size() * 4, finfo, pd.name + ".frag"),
        &res);
    check(res, ctxs + ": createShaderModule(fragment)");
    std::shared_ptr<IShaderStages> stages =
        gpu.device->createShaderStages(ShaderStagesDesc::fromRenderModules(gpu.vertexModule, fragModule), &res);
    check(res, ctxs + ": createShaderStages");

    pg.width = std::max<uint32_t>(1, static_cast<uint32_t>(std::lround(scenario.width * pd.scale)));
    pg.height = std::max<uint32_t>(1, static_cast<uint32_t>(std::lround(scenario.height * pd.scale)));
    TextureDesc td = TextureDesc::new2D(
        pg.format->format, pg.width, pg.height,
        TextureDesc::TextureUsageBits::Sampled | TextureDesc::TextureUsageBits::Attachment,
        pd.name.c_str());
    pg.output = gpu.device->createTexture(td, &res);
    check(res, ctxs + ": createTexture(output " + pd.format + ")");

    FramebufferDesc fbd;
    fbd.colorAttachments[0].texture = pg.output;
    fbd.debugName = pd.name + ".fb";
    pg.framebuffer = gpu.device->createFramebuffer(fbd, &res);
    check(res, ctxs + ": createFramebuffer");

    RenderPipelineDesc rpd;
    rpd.shaderStages = stages;
    rpd.targetDesc.colorAttachments.push_back({.textureFormat = pg.format->format});
    rpd.cullMode = CullMode::Disabled;
    rpd.debugName = genNameHandle(pd.name);
    pg.pipeline = gpu.device->createRenderPipeline(rpd, &res);
    check(res, ctxs + ": createRenderPipeline");

    pg.sampler = pd.sampler == "nearest" ? gpu.samplerNearest : gpu.samplerLinear;
    produced[pd.name] = pg.output;
    gpu.passes.push_back(std::move(pg));
  }

  // ---- timing ----
  const uint32_t P = static_cast<uint32_t>(gpu.passes.size());
  const uint32_t K = static_cast<uint32_t>(job.iterations);
  const uint32_t slots = P * K;
  gpu.timestamps = gpu.device->createTimestampQueries(slots, &res);
  check(res, "createTimestampQueries(" + std::to_string(slots) + ")");
  if (!gpu.timestamps || !gpu.timestamps->isValid() || gpu.timestamps->capacity() < slots) {
    fail("timestamp queries unavailable or too small (need " + std::to_string(slots) + " slots)");
  }
  gpu.timestamps->setTimingFidelity(TimestampQueryFidelity::Accurate);

  for (const auto& pg : gpu.passes) {
    out.timingsNs[pg.desc->name] = {};
  }
  const int totalSubmits = job.warmup + job.samples;
  for (int s = 0; s < totalSubmits; ++s) {
    gpu.timestamps->reset();
    CommandBufferDesc cbd;
    cbd.debugName = "sample " + std::to_string(s);
    auto cmd = gpu.queue->createCommandBuffer(cbd, &res);
    check(res, "createCommandBuffer");
    for (uint32_t it = 0; it < K; ++it) {
      for (uint32_t p = 0; p < P; ++p) {
        PassGpu& pg = gpu.passes[p];
        RenderPassDesc rp;
        rp.colorAttachments.push_back({.loadAction = toLoad(pg.desc->load),
                                       .storeAction = toStore(pg.desc->store),
                                       .clearColor = {0.0f, 0.0f, 0.0f, 0.0f}});
        rp.timestampQuery.queries = gpu.timestamps;
        rp.timestampQuery.slotIndex = it * P + p;
        auto enc = cmd->createRenderCommandEncoder(rp, pg.framebuffer, &res);
        check(res, "createRenderCommandEncoder(" + pg.desc->name + ")");
        enc->bindViewport({0.0f, 0.0f, static_cast<float>(pg.width), static_cast<float>(pg.height), 0.0f, 1.0f});
        enc->bindScissorRect({0, 0, pg.width, pg.height});
        enc->bindRenderPipelineState(pg.pipeline);
        for (const auto& [binding, tex] : pg.textures) {
          enc->bindTexture(binding, BindTarget::kFragment, tex.get());
          enc->bindSamplerState(binding, BindTarget::kFragment, pg.sampler.get());
        }
        if (pg.ubo) {
          enc->bindBuffer(pg.iface.block.binding, BindTarget::kFragment, pg.ubo.get(), 0,
                          pg.ubo->getSizeInBytes());
        }
        enc->draw(3);
        enc->endEncoding();
      }
    }
    gpu.queue->submit(*cmd);
    cmd->waitUntilCompleted();
    if (!gpu.timestamps->resultsAvailable()) {
      fail("timestamp results not available after waitUntilCompleted (sample " + std::to_string(s) + ")");
    }
    if (s < job.warmup) {
      continue;
    }
    for (uint32_t p = 0; p < P; ++p) {
      double total = 0.0;
      for (uint32_t it = 0; it < K; ++it) {
        const TimestampQueryResult r = gpu.timestamps->getElapsedNanosResult(it * P + p);
        if (!r.valid) {
          fail("timestamp slot " + std::to_string(it * P + p) + " invalid in sample " + std::to_string(s));
        }
        total += static_cast<double>(r.elapsedNanos);
      }
      out.timingsNs[gpu.passes[p].desc->name].push_back(total / static_cast<double>(K));
    }
    out.sampleClockMhz.push_back(gpuClockMhzNow());
  }

  // ---- readback (after the final sample) ----
  if (job.readback == "last") {
    for (const PassGpu& pg : gpu.passes) {
      const uint32_t bpp = pg.format->bytesPerPixel;
      const size_t rowBytes = size_t(pg.width) * bpp;
      std::vector<uint8_t> raw(rowBytes * pg.height);
      // IGL's Vulkan copyBytesColorAttachment copies the image and flips it vertically
      // (VulkanStagingDevice::getImageData2D(..., flipImageVertical = true)), so row 0 of `raw`
      // is the bottom texel row. Undo that to get row 0 = top.
      pg.framebuffer->copyBytesColorAttachment(
          *gpu.queue, 0, raw.data(), TextureRangeDesc::new2D(0, 0, pg.width, pg.height), rowBytes);
      std::vector<uint8_t> topDown(raw.size());
      for (uint32_t y = 0; y < pg.height; ++y) {
        std::memcpy(topDown.data() + size_t(y) * rowBytes,
                    raw.data() + size_t(pg.height - 1 - y) * rowBytes, rowBytes);
      }
      NpyImage img;
      img.width = pg.width;
      img.height = pg.height;
      img.data.resize(size_t(pg.width) * pg.height * 4);
      decodeToFloatRGBA(*pg.format, topDown.data(), size_t(pg.width) * pg.height, img.data.data());
      out.images[pg.desc->name] = std::move(img);
    }
  }
  return out;
}

} // namespace shaderlab

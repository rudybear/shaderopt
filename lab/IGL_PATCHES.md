# IGL patches and upstreamable ideas

IGL at `~/sources/igl` is read-only for the lab. Anything the lab needs from IGL is listed here as a separate, upstreamable change; the lab works around it until then.

| # | Need | Status | Workaround in the lab |
|---|---|---|---|
| 1 | Enable `VK_KHR_shader_float_controls2` at device creation (`VulkanFeatures` has no `VkPhysicalDeviceShaderFloatControls2FeaturesKHR` struct) | idea, not written | Runner reports `enabled_by_igl.shaderFloatControls2=false`; M4 fast-math experiments need this patch or a lab-side device creation path |
| 2 | Honor the SPIR-V `DescriptorSet` index instead of hardwiring set 0 = textures, set 1 = buffers | not planned | Runner rewrites the uniform block's `DescriptorSet` decoration to 1 (decoration-only) |
| 3 | `--headless` shell mode crashes on NVIDIA 580.178.04 (driver bug with `VK_EXT_headless_surface`) | driver bug, not IGL | Runner creates the device with no surface and zero swapchain size |

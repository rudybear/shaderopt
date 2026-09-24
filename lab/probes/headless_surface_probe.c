#include <vulkan/vulkan.h>
#include <stdio.h>
#include <string.h>
int main(){
  const char* exts[]={"VK_KHR_surface","VK_EXT_headless_surface"};
  VkApplicationInfo ai={.sType=VK_STRUCTURE_TYPE_APPLICATION_INFO,.apiVersion=VK_API_VERSION_1_3};
  VkInstanceCreateInfo ci={.sType=VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,.pApplicationInfo=&ai,.enabledExtensionCount=2,.ppEnabledExtensionNames=exts};
  VkInstance inst; if(vkCreateInstance(&ci,0,&inst)!=VK_SUCCESS){puts("instance fail");return 1;}
  PFN_vkCreateHeadlessSurfaceEXT mk=(PFN_vkCreateHeadlessSurfaceEXT)vkGetInstanceProcAddr(inst,"vkCreateHeadlessSurfaceEXT");
  VkHeadlessSurfaceCreateInfoEXT hi={.sType=VK_STRUCTURE_TYPE_HEADLESS_SURFACE_CREATE_INFO_EXT};
  VkSurfaceKHR surf; VkResult r=mk(inst,&hi,0,&surf); printf("create headless surface: %d\n",r);
  uint32_t n=0; vkEnumeratePhysicalDevices(inst,&n,0); VkPhysicalDevice pd[8]; vkEnumeratePhysicalDevices(inst,&n,pd);
  for(uint32_t i=0;i<n;i++){ VkPhysicalDeviceProperties p; vkGetPhysicalDeviceProperties(pd[i],&p); printf("[%u] %s: ",i,p.deviceName); fflush(stdout);
    VkSurfaceCapabilitiesKHR caps; r=vkGetPhysicalDeviceSurfaceCapabilitiesKHR(pd[i],surf,&caps);
    printf("caps=%d minImages=%u maxExtent=%ux%u\n",r,caps.minImageCount,caps.maxImageExtent.width,caps.maxImageExtent.height); fflush(stdout);
    uint32_t fc=0; vkGetPhysicalDeviceSurfaceFormatsKHR(pd[i],surf,&fc,0); printf("    formats=%u\n",fc); fflush(stdout); }
  return 0; }

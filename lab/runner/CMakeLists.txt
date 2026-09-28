cmake_minimum_required(VERSION 3.19)
project(shaderlab_runner CXX C)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Release CACHE STRING "" FORCE)
endif()

set(IGL_DIR "/home/rudybear/sources/igl" CACHE PATH "Meta IGL checkout (never modified by this project)")
if(NOT EXISTS "${IGL_DIR}/CMakeLists.txt")
  message(FATAL_ERROR "IGL not found at ${IGL_DIR}; set -DIGL_DIR=...")
endif()

# IGL configuration: Vulkan backend only, no shell/samples/IGLU/tests, deps already deployed.
set(IGL_WITH_VULKAN   ON  CACHE BOOL "" FORCE)
set(IGL_WITH_OPENGL   OFF CACHE BOOL "" FORCE)
set(IGL_WITH_OPENGLES OFF CACHE BOOL "" FORCE)
set(IGL_WITH_METAL    OFF CACHE BOOL "" FORCE)
set(IGL_WITH_WEBGL    OFF CACHE BOOL "" FORCE)
set(IGL_WITH_D3D12    OFF CACHE BOOL "" FORCE)
set(IGL_WITH_IGLU     OFF CACHE BOOL "" FORCE)
set(IGL_WITH_SHELL    OFF CACHE BOOL "" FORCE)
set(IGL_WITH_SAMPLES  OFF CACHE BOOL "" FORCE)
set(IGL_WITH_TESTS    OFF CACHE BOOL "" FORCE)
set(IGL_WITH_TRACY    OFF CACHE BOOL "" FORCE)
set(IGL_WITH_OPENXR   OFF CACHE BOOL "" FORCE)
set(IGL_ENFORCE_LOGS  ON  CACHE BOOL "" FORCE)   # validation messages arrive through IGL logging
set(IGL_DEPLOY_DEPS   OFF CACHE BOOL "" FORCE)   # do not re-run deploy_deps.py
if(ANDROID)
  # Android cross-build (build-android.sh): IGL's own CMake forces Vulkan on, OpenGL off; there is no
  # shell, no window and no surface, exactly like the desktop runner. IGL's Vulkan backend loads
  # libvulkan.so through volk at runtime; liblog/libandroid are needed by IGL's Android log handler,
  # AHardwareBuffer support (API >= 26) and the ATrace profiler macros.
  set(IGL_WITH_OPENGLES OFF CACHE BOOL "" FORCE)
  if(NOT ANDROID_ABI STREQUAL "arm64-v8a")
    message(WARNING "shaderlab-runner is only tested for arm64-v8a (got ${ANDROID_ABI})")
  endif()
endif()
add_subdirectory(${IGL_DIR} igl EXCLUDE_FROM_ALL)

# Tool identity baked into result.json
execute_process(COMMAND git -C ${IGL_DIR} rev-parse HEAD
                OUTPUT_VARIABLE SHADERLAB_IGL_COMMIT OUTPUT_STRIP_TRAILING_WHITESPACE ERROR_QUIET)
execute_process(COMMAND git -C ${CMAKE_CURRENT_SOURCE_DIR} rev-parse --short HEAD
                OUTPUT_VARIABLE SHADERLAB_REPO_HEAD OUTPUT_STRIP_TRAILING_WHITESPACE ERROR_QUIET)
string(TIMESTAMP SHADERLAB_BUILD_TIME "%Y-%m-%dT%H:%M:%SZ" UTC)
if(NOT SHADERLAB_REPO_HEAD)
  set(SHADERLAB_REPO_HEAD "nogit")
endif()
set(SHADERLAB_RUNNER_BUILD "${SHADERLAB_REPO_HEAD}@${SHADERLAB_BUILD_TIME}")

set(SPIRV_REFLECT_DIR "${IGL_DIR}/third-party/deps/src/gfxreconstruct/external/SPIRV-Reflect")
if(NOT EXISTS "${SPIRV_REFLECT_DIR}/spirv_reflect.c")
  message(FATAL_ERROR "SPIRV-Reflect not found at ${SPIRV_REFLECT_DIR}")
endif()

add_executable(shaderlab-runner
  src/main.cpp
  src/runner.cpp
  src/scenario.cpp
  src/reflect.cpp
  src/formats.cpp
  src/npy.cpp
  src/sysinfo.cpp
  src/validation.cpp
  ${SPIRV_REFLECT_DIR}/spirv_reflect.c)
target_include_directories(shaderlab-runner PRIVATE
  ${CMAKE_CURRENT_SOURCE_DIR}/third_party
  ${CMAKE_CURRENT_SOURCE_DIR}/third_party/nlohmann_shim
  ${SPIRV_REFLECT_DIR})
target_compile_definitions(shaderlab-runner PRIVATE
  SHADERLAB_IGL_COMMIT="${SHADERLAB_IGL_COMMIT}"
  SHADERLAB_RUNNER_BUILD="${SHADERLAB_RUNNER_BUILD}")
target_link_libraries(shaderlab-runner PRIVATE IGLLibrary IGLVulkan)
if(ANDROID)
  target_link_libraries(shaderlab-runner PRIVATE log android)
  # Runs from /data/local/tmp as the shell user: no app sandbox, no libc++_shared.so next to it.
  if(NOT ANDROID_STL STREQUAL "c++_static")
    message(FATAL_ERROR "build with -DANDROID_STL=c++_static so the binary runs from /data/local/tmp")
  endif()
endif()
set_source_files_properties(${SPIRV_REFLECT_DIR}/spirv_reflect.c PROPERTIES LANGUAGE C)

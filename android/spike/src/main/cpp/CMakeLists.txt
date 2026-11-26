# Qwen3-ASR on the phone through transcribe.cpp (MIT), the engine the desktop
# app uses. The source is downloaded at build time (not kept in git): the same
# version as the desktop's transcribe-cpp crate. CPU backend only (arm64 NEON,
# dot product, i8mm); a GPU backend is left out of the spike.
cmake_minimum_required(VERSION 3.22)
project(seogi_spike_asr C CXX)

include(FetchContent)
set(TRANSCRIBE_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(TRANSCRIBE_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(TRANSCRIBE_BUILD_TOOLS OFF CACHE BOOL "" FORCE)
set(TRANSCRIBE_BUILD_SHARED OFF CACHE BOOL "" FORCE)
set(TRANSCRIBE_USE_SYSTEM_BLAS OFF CACHE BOOL "" FORCE)
set(TRANSCRIBE_METAL OFF CACHE BOOL "" FORCE)
set(TRANSCRIBE_INSTALL OFF CACHE BOOL "" FORCE)
set(GGML_NATIVE OFF CACHE BOOL "" FORCE)
set(GGML_OPENMP OFF CACHE BOOL "" FORCE)
# Snapdragon 8 Elite (and every phone from the last few years) has these.
set(GGML_CPU_ARM_ARCH "armv8.6-a+dotprod+fp16+i8mm" CACHE STRING "" FORCE)

# tools/fetch.sh puts the tarball in the spike cache; build.gradle.kts passes
# it here. Without it CMake downloads the same file (same checksum).
if(SEOGI_TRANSCRIBE_TARBALL AND EXISTS "${SEOGI_TRANSCRIBE_TARBALL}")
    set(_tc_url "${SEOGI_TRANSCRIBE_TARBALL}")
else()
    set(_tc_url https://github.com/handy-computer/transcribe.cpp/archive/refs/tags/v0.4.2.tar.gz)
endif()
FetchContent_Declare(transcribe
    URL ${_tc_url}
    URL_HASH SHA256=5455374e18adaa0d79ff48104b13109d15dd6002edda999eda1179a86ea09c5b
    INACTIVITY_TIMEOUT 60)
FetchContent_MakeAvailable(transcribe)
# Its src/CMakeLists.txt finds the public headers through CMAKE_SOURCE_DIR,
# which is this folder when it is built as a part.
target_include_directories(transcribe PUBLIC ${transcribe_SOURCE_DIR}/include)

add_library(seogi_asr SHARED transcribe_jni.c)
target_link_libraries(seogi_asr PRIVATE transcribe log)

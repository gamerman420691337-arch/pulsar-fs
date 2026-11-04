#!/usr/bin/env bash
# Build-only tools; the resulting candidate does not qualify an edition or model.
# Same pinned CPU baseline as the text assistant, with a dedicated vision CLI.
set -euo pipefail
if [[ $# -ne 2 || $1 != /* || $2 != /* ]]; then
  echo "usage: build-linux-vision-runtime.sh /absolute/pinned-source /absolute/new-build" >&2
  exit 2
fi
source_root=$1
build_root=$2
expected_revision=b86d2f07542b29ab099aed34fd6b6d1b2fd4b81c
[[ $(uname -s) == Linux && $(uname -m) == x86_64 ]]
[[ ! -e $build_root ]]
[[ $(timeout 10 git -C "$source_root" rev-parse HEAD) == "$expected_revision" ]]
[[ -z $(timeout 10 git -C "$source_root" status --porcelain) ]]
timeout 120 cmake -S "$source_root" -B "$build_root" -G Ninja \
  -DCMAKE_MAKE_PROGRAM=/usr/bin/ninja -DCMAKE_C_COMPILER=/usr/bin/gcc -DCMAKE_CXX_COMPILER=/usr/bin/g++ \
  -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_FLAGS="-march=x86-64 -mtune=generic" \
  -DCMAKE_CXX_FLAGS="-march=x86-64 -mtune=generic" -DCMAKE_EXE_LINKER_FLAGS=-static \
  -DBUILD_SHARED_LIBS=OFF -DGGML_BACKEND_DL=OFF -DGGML_NATIVE=OFF -DGGML_OPENMP=OFF \
  -DGGML_BLAS=OFF -DGGML_CUDA=OFF -DGGML_VULKAN=OFF -DGGML_SYCL=OFF \
  -DGGML_SSE42=OFF -DGGML_AVX=OFF -DGGML_AVX2=OFF -DGGML_AVX_VNNI=OFF \
  -DGGML_BMI2=OFF -DGGML_FMA=OFF -DGGML_F16C=OFF -DGGML_AVX512=OFF \
  -DGGML_AVX512_VBMI=OFF -DGGML_AVX512_VNNI=OFF -DGGML_AVX512_BF16=OFF \
  -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_SERVER=OFF \
  -DLLAMA_BUILD_APP=OFF -DLLAMA_BUILD_UI=OFF -DLLAMA_BUILD_TOOLS=ON \
  -DLLAMA_OPENSSL=OFF -DLLAMA_SUBPROCESS=OFF -DMTMD_VIDEO=OFF
 timeout 600 cmake --build "$build_root" --target llama-mtmd-cli --parallel 4
 timeout 10 readelf -h "$build_root/bin/llama-mtmd-cli"
 headers=$(timeout 10 readelf -l "$build_root/bin/llama-mtmd-cli")
 if [[ $headers == *INTERP* ]]; then
   echo "static vision runtime unexpectedly has an interpreter" >&2; exit 1
 fi
 dynamic=$(timeout 10 readelf -d "$build_root/bin/llama-mtmd-cli")
 if [[ $dynamic == *NEEDED* ]]; then
   echo "static vision runtime unexpectedly has shared-library dependencies" >&2; exit 1
 fi
 timeout 10 "$build_root/bin/llama-mtmd-cli" --version

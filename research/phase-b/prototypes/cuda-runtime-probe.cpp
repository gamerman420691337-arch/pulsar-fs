// Authored host diagnostic only. Not a production worker or GPU qualification.
#include <onnxruntime/onnxruntime_cxx_api.h>
#include <algorithm>
#include <array>
#include <cmath>
#include <fstream>
#include <iostream>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>

int main(int argc, char** argv) {
  try {
    if (argc != 4 || (std::string(argv[1]) != "cpu" && std::string(argv[1]) != "cuda"))
      throw std::runtime_error("usage: cuda-runtime-probe cpu|cuda AUTHORED-MODEL PROFILE-PREFIX");
    const bool cuda = std::string(argv[1]) == "cuda";
    Ort::Env environment(ORT_LOGGING_LEVEL_WARNING, "pulsar-host-diagnostic");
    Ort::SessionOptions options;
    options.SetIntraOpNumThreads(2);
    options.SetInterOpNumThreads(1);
    options.SetGraphOptimizationLevel(GraphOptimizationLevel::ORT_ENABLE_ALL);
    options.EnableProfiling(argv[3]);
    if (cuda) {
      // Registration alone is insufficient. Refuse nodes assigned to CPU.
      options.AddConfigEntry("session.disable_cpu_ep_fallback", "1");
      Ort::CUDAProviderOptions provider;
      provider.Update({{"device_id", "0"}, {"gpu_mem_limit", "67108864"},
                       {"arena_extend_strategy", "kSameAsRequested"}, {"use_tf32", "0"}});
      options.AppendExecutionProvider_CUDA_V2(*provider);
    }
    Ort::Session session(environment, argv[2], options);
    Ort::AllocatorWithDefaultOptions allocator;
    if (session.GetInputCount() != 1 || session.GetOutputCount() != 1)
      throw std::runtime_error("authored model tensor count differs");
    auto input_name = session.GetInputNameAllocated(0, allocator);
    auto output_name = session.GetOutputNameAllocated(0, allocator);
    if (std::string(input_name.get()) != "frame_pair" || std::string(output_name.get()) != "script_position")
      throw std::runtime_error("authored model tensor names differ");
    const std::array<int64_t, 4> shape{1, 6, 64, 64};
    auto memory = Ort::MemoryInfo::CreateCpu(OrtArenaAllocator, OrtMemTypeDefault);
    const char* input_names[]{input_name.get()};
    const char* output_names[]{output_name.get()};
    std::vector<float> pixels(6 * 64 * 64);
    std::array<float, 4> outputs{};
    const std::array<float, 4> levels{0.f, .25f, .5f, 1.f};
    for (size_t i = 0; i < levels.size(); ++i) {
      std::fill(pixels.begin(), pixels.end(), levels[i]);
      auto tensor = Ort::Value::CreateTensor<float>(memory, pixels.data(), pixels.size(), shape.data(), shape.size());
      auto result = session.Run(Ort::RunOptions{nullptr}, input_names, &tensor, 1, output_names, 1);
      const auto info = result.at(0).GetTensorTypeAndShapeInfo();
      if (info.GetShape() != std::vector<int64_t>{1, 1} || info.GetElementType() != ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT)
        throw std::runtime_error("authored model output contract differs");
      outputs[i] = result[0].GetTensorData<float>()[0];
      if (!std::isfinite(outputs[i]) || std::abs(outputs[i] - (.25f + .5f * levels[i])) > 1e-5f)
        throw std::runtime_error("independent authored output oracle failed");
    }
    auto profile = session.EndProfilingAllocated(allocator);
    std::cout << "{\"scope\":\"research_diagnostic_no_production_authority\",\"provider_requested\":\""
              << argv[1] << "\",\"runs\":4,\"outputs\":[";
    for (size_t i = 0; i < outputs.size(); ++i) std::cout << (i ? "," : "") << outputs[i];
    // Prefix is supplied by the local harness, not an arbitrary JSON string.
    // Profile path is emitted separately so the harness can parse it as text.
    std::cout << "],\"whole_worker_vram_quota_enforced\":false,\"production_integrated\":false}\n";
    std::cerr << "PROFILE_PATH=" << profile.get() << '\n';
    std::ifstream maps("/proc/self/maps");
    if (!maps) throw std::runtime_error("loaded-library mapping unavailable");
    std::set<std::string> paths;
    std::string line;
    size_t count = 0;
    while (std::getline(maps, line)) {
      if (++count > 16384 || line.size() > 8192)
        throw std::runtime_error("loaded-library mapping exceeds diagnostic bound");
      const auto path = line.find('/');
      if (path != std::string::npos && line.find(".so", path) != std::string::npos)
        paths.insert(line.substr(path));
    }
    for (const auto& path : paths) std::cerr << "LIBRARY_PATH=" << path << '\n';
    return 0;
  } catch (const std::exception& error) {
    std::cerr << "host diagnostic refused: " << error.what() << '\n';
    return 1;
  }
}

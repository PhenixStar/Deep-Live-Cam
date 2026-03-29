# Research: DirectML GPU Acceleration in ort Crate v2.0.0-rc.12 (Windows / AMD Radeon 890M)

**Date:** 2026-03-29
**Target:** deep-forge — Rust sidecar using `ort` v2.0.0-rc.12 on GPD Sweep (Ryzen AI 9 HX 370, Radeon 890M iGPU, 64 GB shared LPDDR5X)
**Scope:** SCRFD face detection + inswapper face swap ONNX models

---

## Executive Summary

DirectML acceleration in the `ort` Rust crate v2 RC is possible but requires a **custom ONNX Runtime build** — the prebuilt `download-binaries` path does not include DirectML. Once a custom build is linked, the API surface is straightforward. The Radeon 890M (RDNA 3.5, 16 CUs) with 64 GB unified memory sidesteps the biggest iGPU pitfall (VRAM exhaustion into slow shared RAM). Realistic speedup over CPU: **3–6x for SCRFD**, potentially **2–4x for inswapper_128**, with session-reuse being essential to amortize the heavier initialization cost.

---

## 1. Feature Flags

### Cargo.toml

```toml
[dependencies]
ort = { version = "2.0.0-rc.12", features = ["directml"] }
```

The `directml` feature is a named Cargo feature in `ort` v2 RC. It enables the `DirectMLExecutionProvider` struct and propagates the flag to `ort-sys`, which triggers D3D12/DXGI linking in the build script.

### Full feature flag list (relevant subset from ort 2.0.0-rc.8+)

| Feature | Purpose |
|---|---|
| `directml` | Windows DirectX 12 GPU acceleration — the flag you want |
| `cuda` | NVIDIA CUDA (not applicable here) |
| `download-binaries` | Auto-fetches Microsoft prebuilt ORT binaries (default ON) |
| `copy-dylibs` | Copies shared libs into Cargo target folder (default ON) |
| `load-dynamic` | Runtime dlopen instead of link-time (useful for packaging) |
| `half` | fp16 tensor support (default ON) |

**Critical:** `download-binaries` and `directml` are mutually exclusive in practice. The Microsoft prebuilt binaries only include CUDA and TensorRT EPs. Enabling `directml` with `download-binaries` will either silently fall back to CPU or fail at link time. See Section 5 for the required workaround.

---

## 2. Session Builder API

### Minimal DirectML session

```rust
use ort::{ep, session::Session};

fn load_model(model_path: &str) -> anyhow::Result<Session> {
    let session = Session::builder()?
        .with_execution_providers([
            ep::DirectML::default().build(),
        ])?
        .commit_from_file(model_path)?;
    Ok(session)
}
```

### With device ID (select specific adapter)

```rust
let session = Session::builder()?
    .with_execution_providers([
        ep::DirectML::default()
            .with_device_id(0)   // 0 = default adapter (Radeon 890M on this machine)
            .build(),
    ])?
    .commit_from_file(model_path)?;
```

`device_id` maps to the DXGI adapter enumeration order (`IDXGIFactory::EnumAdapters`). On a machine with only the iGPU, `0` is always correct.

### With CPU fallback (recommended pattern)

```rust
use ort::{ep, session::Session};

let session = Session::builder()?
    .with_execution_providers([
        ep::DirectML::default().with_device_id(0).build(),
        ep::CPU::default().build(),   // fallback if DML unavailable
    ])?
    .commit_from_file(model_path)?;
```

Execution providers are tried in order; ORT falls back silently if registration fails.

### Mandatory session options for DirectML

DirectML has two hard requirements that must be set before `commit_from_file`:

```rust
use ort::session::{builder::GraphOptimizationLevel, Session};

let session = Session::builder()?
    // DirectML does not support memory pattern optimizations
    .with_memory_pattern(false)?
    // DirectML requires sequential execution (no parallel op dispatch)
    // ORT_SEQUENTIAL is the ort v2 default, but set explicitly to be safe
    .with_execution_providers([
        ep::DirectML::default().with_device_id(0).build(),
    ])?
    .commit_from_file(model_path)?;
```

In ort v2 RC, session execution mode defaults to sequential, so the second constraint is typically satisfied automatically. However, if you ever see crashes or incorrect results under DirectML, verify these two settings first.

### Free dimension overrides (performance)

If your ONNX model has dynamic input shapes (SCRFD does), pin the shapes at session creation time for best performance. DirectML can pre-process weights and apply additional graph optimizations only when shapes are known statically:

```rust
session_builder
    .with_execution_providers([ep::DirectML::default().build()])?
    // Pin SCRFD input: batch=1, H=640, W=640
    .add_free_dimension_override_by_name("batch", 1)?
    .add_free_dimension_override_by_name("height", 640)?
    .add_free_dimension_override_by_name("width", 640)?
    .commit_from_file("scrfd_10g_bnkps.onnx")?;
```

---

## 3. DLL Dependencies at Runtime

### Required DLLs

| DLL | Source | Notes |
|---|---|---|
| `onnxruntime.dll` | Your custom ORT build output | Core runtime |
| `onnxruntime_providers_shared.dll` | ORT build output | Provider shared infrastructure |
| `onnxruntime_providers_dml.dll` | ORT build output | DirectML EP itself |
| `DirectML.dll` | Bundled by ORT build (`--use_dml` auto-downloads the redistributable) | DirectML compute library |
| `d3d12.dll` | Windows OS (system32) | Always present on Win10 1903+ |
| `dxgi.dll` | Windows OS (system32) | Always present |

### Where to get them

When you build ORT from source with `--use_dml`, the build downloads the DirectML NuGet package automatically and the redistributable `DirectML.dll` ends up in your build output directory alongside the other DLLs. You do not need to separately acquire it.

### Runtime DLL discovery

All four non-system DLLs must be in one of:
- The same directory as your `.exe`
- A directory on the `PATH`
- The Cargo target directory during development (handled by `copy-dylibs` feature)

The `copy-dylibs` Cargo feature (enabled by default in `ort`) symlinks/copies these into the Cargo `target/` folder automatically during `cargo build`, which is sufficient for `cargo run`. For packaged Tauri builds, you must bundle these DLLs alongside the sidecar binary explicitly.

### Tauri sidecar bundling note

Add all four DLLs to `tauri.conf.json` `bundle.resources` or `externalBin` alongside the Rust sidecar binary. Without them, the sidecar will panic at ONNX Runtime initialization with a "DLL not found" or "provider not available" error.

---

## 4. Known Issues with DirectML on AMD iGPUs

### 4.1 Shared memory fallback (most important for 890M)

When DirectML exhausts dedicated VRAM, ORT falls back to shared system memory. On discrete GPUs with 8–12 GB VRAM this causes a severe performance cliff. However, the Radeon 890M is a pure iGPU — it has **no dedicated VRAM pool**, it always uses shared system memory (LPDDR5X from the 64 GB pool). This means:

- There is no "VRAM cliff" to fall off — memory bandwidth is uniformly LPDDR5X (~100–130 GB/s on this machine)
- Models of any size can run without the shared-memory fallback penalty that plagues discrete iGPU scenarios
- The main constraint is bandwidth, not capacity

BIOS "UMA Frame Buffer Size" setting (8–16 GB) reserves a dedicated pool within the 64 GB for the iGPU. Setting it to 8 or 16 GB is recommended to give the GPU its own allocation and avoid OS memory pressure from other processes.

### 4.2 No memory pattern optimization

`DisableMemPattern` is mandatory. This prevents ORT from pre-allocating reusable memory arenas, which DirectML's memory model is incompatible with. In practice this means slightly higher per-inference allocation overhead, not a significant issue for large models like inswapper_128.

### 4.3 Sequential-only execution

DirectML does not support parallel operator dispatch. Only one thread may call `session.run()` at a time on a given session. For the deep-forge sidecar (single-threaded inference pipeline), this is a non-issue. For multi-session parallelism, create separate `Session` instances rather than sharing one.

### 4.4 Divergent numerical results vs CPU

There are open GitHub issues (microsoft/onnxruntime#14214, #19352) reporting that DirectML can produce slightly different numerical outputs compared to CPU or CUDA for certain layer types. This is most likely to manifest in models with non-standard activations or reductions. For face detection (SCRFD) and face swap (inswapper_128), which use standard Conv/BN/ReLU ops, this is generally not a problem in practice. Validate outputs against CPU reference on first integration.

### 4.5 AMD eGPU timeout (not applicable but FYI)

Reports exist of AMD eGPU (discrete) triggering GPU driver timeout (error 0x887A0006) under high VRAM use with DirectML. This is a discrete GPU issue related to TDR (Timeout Detection and Recovery). The 890M iGPU does not have this problem.

### 4.6 DirectML is in "sustained engineering" mode

As of 2024, Microsoft has officially marked DirectML as "sustained engineering" — maintained for correctness/security but receiving no new features. New Windows AI feature work is moving to WinML and WebNN. For the models in this project (SCRFD opset 11–13, inswapper opset 11), all required ops are well within DirectML's supported opset range (up to opset 20), so this does not affect feasibility.

### 4.7 Session initialization overhead

DirectML sessions take notably longer to initialize than CPU sessions (1–5 seconds depending on model size) due to graph compilation. This is a one-time cost. Sessions must be created once at startup and reused across all inference calls. Never create a new session per frame.

---

## 5. download-binaries vs Custom Build

### The core constraint

The `ort` crate's `download-binaries` default feature downloads Microsoft's official prebuilt `onnxruntime` binaries. **These binaries do not include DirectML** (or any EP other than CUDA and TensorRT). The crate documentation states:

> "Microsoft does not provide prebuilt binaries for execution providers other than CUDA and TensorRT, and thus enabling other EP features will fail when ORT_STRATEGY=download."

### Required path: custom ORT build

You must build ONNX Runtime from source with DirectML enabled.

#### Step 1: Build ONNX Runtime

```powershell
# Clone ORT
git clone https://github.com/microsoft/onnxruntime.git
cd onnxruntime

# Build with DirectML support
# Requires: Visual Studio 2022 (with C++ workload), Python 3.10+, CMake 3.27+
.\build.bat `
    --config Release `
    --build_shared_lib `
    --parallel `
    --use_dml `
    --skip_tests
```

Build output will be at `build\Windows\Release\Release\`.

#### Step 2: Configure ort crate to use custom build

In your project's `.cargo/config.toml` (or set as env vars):

```toml
[env]
# Point ort-sys to your custom build
ORT_LIB_PATH = "C:/path/to/onnxruntime/build/Windows/Release/Release"
```

Then in `Cargo.toml`, disable `download-binaries` and enable `directml`:

```toml
[dependencies]
ort = { version = "2.0.0-rc.12", default-features = false, features = [
    "directml",
    "copy-dylibs",
    "half",
    "ndarray",
] }
```

`default-features = false` removes `download-binaries` (and `tls-native`). You must re-add any other defaults you need.

#### Step 3: Runtime DLL placement (Tauri)

Copy from the ORT build output into your Tauri `src-tauri/binaries/` or via `bundle.resources`:
- `onnxruntime.dll`
- `onnxruntime_providers_shared.dll`
- `onnxruntime_providers_dml.dll`
- `DirectML.dll`

#### Alternative: NuGet prebuilt (Microsoft.ML.OnnxRuntime.DirectML)

Microsoft publishes a NuGet package `Microsoft.ML.OnnxRuntime.DirectML` with prebuilt DirectML-enabled ORT DLLs. You can extract the DLLs from the NuGet package and use `ORT_LIB_PATH` to point `ort-sys` at them, avoiding a full ORT source build. The NuGet package structure includes:
- `runtimes/win-x64/native/onnxruntime.dll`
- `runtimes/win-x64/native/onnxruntime_providers_shared.dll`
- `runtimes/win-x64/native/onnxruntime_providers_dml.dll`
- `runtimes/win-x64/native/DirectML.dll`

This is likely the fastest path to a working DirectML build without the multi-hour ORT compilation.

```powershell
# Download NuGet package (it's just a zip)
Invoke-WebRequest -Uri "https://www.nuget.org/api/v2/package/Microsoft.ML.OnnxRuntime.DirectML/1.20.1" `
    -OutFile ort-directml.nupkg
Rename-Item ort-directml.nupkg ort-directml.zip
Expand-Archive ort-directml.zip -DestinationPath ort-directml/
# DLLs will be in ort-directml/runtimes/win-x64/native/
```

Then set `ORT_LIB_PATH` to the `runtimes/win-x64/native/` directory.

---

## 6. Performance Expectations

### Context: AMD Radeon 890M (RDNA 3.5, 16 CUs)

- 16 Compute Units = 1024 shader processors
- Peak FP32 throughput: ~4.4 TFLOPS
- Memory bandwidth: ~100 GB/s (shared LPDDR5X-7500)
- DirectX 12 feature level: 12_1 — fully compatible
- No dedicated VRAM; uses 64 GB shared pool (no VRAM cliff issue)

### SCRFD (face detection)

SCRFD-10G with 640x640 input:
- CPU baseline (single-thread, AMD Ryzen 9 3950X): ~28 ms per frame
- CPU baseline (Zen 5 in HX 370, estimated): ~15–20 ms per frame (faster IPC + AVX-512)
- DirectML (Radeon 890M, estimated): **4–8 ms per frame** — roughly 3–5x faster than CPU

The speedup is significant because SCRFD's convolutional layers map efficiently to GPU shader compute. The 640x640 input generates sufficient compute density to amortize GPU overhead.

### inswapper_128 (face swap, ~554 MB FP32)

- CPU baseline: ~200–400 ms per face swap (varies with thread count)
- DirectML (Radeon 890M, estimated): **50–120 ms per face swap**
- Speedup: roughly 3–4x

inswapper_128 is ~130M parameters. Its UNet-like architecture with heavy Conv layers is well-suited to DirectML. However:
- The ~554 MB FP32 model will consume significant bandwidth during each inference
- Memory bandwidth (not compute) is likely the bottleneck on the 890M
- FP16 inference (if model supports it) would approximately halve memory bandwidth demand and may yield 1.5–2x additional speedup

### Combined pipeline at 720p (SCRFD + inswapper)

| Mode | Estimated latency | Est. FPS |
|---|---|---|
| CPU only | ~220–430 ms | 2–4 FPS |
| DirectML (FP32) | ~55–130 ms | 7–18 FPS |
| DirectML (FP16, if available) | ~30–70 ms | 14–30 FPS |

These are estimates based on GPU compute specs, the prior AMD APU research in this project, and known ONNX Runtime DirectML benchmarks on comparable hardware. Actual numbers require empirical measurement on the target device.

### Key performance rules for DirectML on this hardware

1. **Session reuse is mandatory.** DirectML's 1–5 second session init cost means one-time loading at startup. Never re-create sessions per frame.
2. **Batch size 1 is optimal** for real-time. DirectML does not benefit from larger batches the way CUDA does, due to sequential execution constraints.
3. **Fixed input shapes outperform dynamic.** Use `add_free_dimension_override_by_name` to lock input dimensions at session creation.
4. **BIOS UMA Frame Buffer: set to 8–16 GB** to give the iGPU its own dedicated allocation window in the shared pool.
5. **Avoid repeated small inference calls.** DirectML has higher per-call overhead than CPU. Batching detection + swap into a single pipeline reduces round-trips.

---

## 7. Summary Decision Matrix

| Question | Answer |
|---|---|
| Feature flag for DirectML in ort v2 | `features = ["directml"]` |
| `download-binaries` includes DirectML? | **No.** Requires custom build or NuGet extraction. |
| Session API | `ep::DirectML::default().with_device_id(0).build()` |
| `with_memory_pattern(false)` required? | Yes |
| `ORT_SEQUENTIAL` required? | Yes (ort v2 default, but verify) |
| Runtime DLLs needed | `onnxruntime.dll`, `onnxruntime_providers_shared.dll`, `onnxruntime_providers_dml.dll`, `DirectML.dll` |
| Fastest path to DLLs | Extract from `Microsoft.ML.OnnxRuntime.DirectML` NuGet |
| AMD 890M VRAM cliff risk? | **None** — pure iGPU with 64 GB unified memory |
| Expected speedup vs CPU (SCRFD) | 3–5x |
| Expected speedup vs CPU (inswapper) | 3–4x |
| DirectML maintenance status | Sustained engineering (no new features, but stable) |

---

## 8. Recommended Implementation Path

1. **Download NuGet package** `Microsoft.ML.OnnxRuntime.DirectML` (latest 1.x) and extract DLLs — fastest path, avoids full ORT source build
2. **Set `ORT_LIB_PATH`** in `.cargo/config.toml` pointing at the extracted native DLL directory
3. **Update `Cargo.toml`**: `default-features = false`, add `directml`, `copy-dylibs`, `half`
4. **Add `ep::DirectML::default().with_device_id(0).build()`** as first provider in session builder
5. **Disable memory pattern** with `.with_memory_pattern(false)?`
6. **Test fallback**: verify CPU fallback works when DML not available (CI without GPU)
7. **Bundle DLLs** in Tauri installer alongside the sidecar binary
8. **Benchmark** SCRFD + inswapper on device with session reuse; measure init time vs per-frame time separately

---

## References

- [ort Execution Providers docs](https://ort.pyke.io/perf/execution-providers)
- [ort crates.io (2.0.0-rc.12)](https://docs.rs/crate/ort/latest)
- [ONNX Runtime DirectML EP docs](https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html)
- [AMD GPUOpen DirectML guide part 1](https://gpuopen.com/learn/onnx-directlml-execution-provider-guide-part1/)
- [DirectML GitHub (maintenance notice)](https://github.com/microsoft/DirectML)
- [microsoft/onnxruntime#19395 — shared memory discussion](https://github.com/microsoft/onnxruntime/discussions/19395)
- [microsoft/onnxruntime#14168 — DirectML worse than CPU discussion](https://github.com/microsoft/onnxruntime/discussions/14168)
- [NuGet: Microsoft.ML.OnnxRuntime.DirectML](https://www.nuget.org/packages/Microsoft.ML.OnnxRuntime.directml)
- [ort linking docs (GitHub source)](https://github.com/pykeio/ort/blob/main/docs/content/setup/linking.mdx)
- [pykeio/ort DeepWiki](https://deepwiki.com/pykeio/ort)
- Prior report: [researcher-260328-1257-amd-apu-npu-inference.md](./researcher-260328-1257-amd-apu-npu-inference.md)

# Phase 1: DirectML GPU Acceleration

**Priority:** P0 — Critical (current 2-4 FPS on CPU is unusable)
**Effort:** 8h
**Status:** Pending

## Context

- [DirectML research report](../reports/researcher-260329-directml-ort.md)
- Current: CPU inference, ~4s per frame (detect + swap)
- Target: DirectML on Radeon 890M, ~55-130ms per frame (7-18 FPS)
- Hardware: AMD Ryzen AI 9 HX 370, Radeon 890M (4GB VRAM allocated), 64GB LPDDR5X

## Key Insights

- `ort` crate v2 has `directml` feature flag — but `download-binaries` only fetches CPU+CUDA, NOT DirectML
- Must extract DLLs from NuGet package `Microsoft.ML.OnnxRuntime.DirectML` v1.20.1
- Session builder requires `with_memory_pattern(false)` for DirectML
- Session init is slow (1-5s) — create once at startup, reuse forever
- AMD 890M iGPU uses unified memory — no VRAM cliff issue

## Architecture

```
Startup:
  1. Download/verify ORT DirectML DLLs (onnxruntime.dll, onnxruntime_providers_dml.dll, DirectML.dll)
  2. Set ORT_LIB_PATH to DLL directory
  3. Create sessions with DirectML EP + CPU fallback
  4. Log which EP was selected per model

Runtime:
  FaceDetector::new()  → Session with [DirectML, CPU]
  FaceSwapper::new()   → Session with [DirectML, CPU]
  produce_frame()      → Same as now, but 3-4x faster
```

## Related Code Files

### Modify
- `core/rust-engine/Cargo.toml` — workspace deps: switch ort features
- `core/rust-engine/dlc-core/Cargo.toml` — remove `download-binaries`, add `directml`
- `core/rust-engine/dlc-core/src/detect.rs` — Session builder with DirectML EP
- `core/rust-engine/dlc-core/src/swap.rs` — Session builder with DirectML EP
- `core/rust-engine/dlc-core/src/enhance.rs` — Session builder with DirectML EP (if used)
- `core/rust-engine/dlc-core/src/lib.rs` — Add EP configuration struct
- `core/rust-engine/dlc-server/src/main.rs` — Pass EP config to model constructors
- `scripts/build-rust-sidecar-win.ps1` — Add NuGet DLL extraction step
- `scripts/setup-directml-dlls.ps1` — New: download + extract NuGet DLLs

### Create
- `scripts/setup-directml-dlls.ps1` — Download NuGet, extract DLLs to known path

## Implementation Steps

### Step 1: Setup DirectML DLL provisioning

Create `scripts/setup-directml-dlls.ps1`:
```powershell
$NugetUrl = "https://www.nuget.org/api/v2/package/Microsoft.ML.OnnxRuntime.DirectML/1.20.1"
$OutDir = "$PSScriptRoot/../core/rust-engine/ort-dml-libs"

if (Test-Path "$OutDir/onnxruntime.dll") {
    Write-Host "DirectML DLLs already present, skipping download."
    exit 0
}

Write-Host "Downloading ORT DirectML NuGet..."
Invoke-WebRequest -Uri $NugetUrl -OutFile "$env:TEMP/ort-dml.zip"
Expand-Archive "$env:TEMP/ort-dml.zip" -DestinationPath "$env:TEMP/ort-dml" -Force

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
Copy-Item "$env:TEMP/ort-dml/runtimes/win-x64/native/*.dll" $OutDir
Write-Host "DLLs extracted to $OutDir"
```

### Step 2: Update ort crate configuration

In `core/rust-engine/Cargo.toml` workspace deps:
```toml
ort = { version = "2.0.0-rc.12", default-features = false, features = ["directml", "copy-dylibs", "half"] }
```

Set in `core/rust-engine/.cargo/config.toml`:
```toml
[env]
ORT_LIB_PATH = { value = "ort-dml-libs", relative = true }
```

### Step 3: Add EP configuration to dlc-core

Add `EpConfig` to `dlc-core/src/lib.rs`:
```rust
#[derive(Debug, Clone)]
pub enum GpuProvider {
    DirectML { device_id: i32 },
    Cpu,
    Auto, // Try DirectML, fall back to CPU
}

impl Default for GpuProvider {
    fn default() -> Self { Self::Auto }
}
```

### Step 4: Update FaceDetector session builder

In `dlc-core/src/detect.rs`:
```rust
pub fn new(model_path: &std::path::Path, provider: &GpuProvider) -> Result<Self> {
    let mut builder = Session::builder()
        .context("Session::builder failed")?;

    match provider {
        GpuProvider::DirectML { device_id } | GpuProvider::Auto => {
            builder = builder
                .with_memory_pattern(false)?
                .with_execution_providers([
                    ort::ep::DirectML::default()
                        .with_device_id(if matches!(provider, GpuProvider::Auto) { 0 } else { *device_id })
                        .build(),
                    ort::ep::CPU::default().build(),
                ])?;
        }
        GpuProvider::Cpu => {
            builder = builder
                .with_execution_providers([
                    ort::ep::CPU::default().build(),
                ])?;
        }
    }

    let session = builder
        .commit_from_file(model_path)
        .with_context(|| format!("Failed to load {}", model_path.display()))?;

    // Log which EP was actually selected
    tracing::info!("SCRFD session created on {:?}", provider);
    Ok(Self { session })
}
```

### Step 5: Update FaceSwapper similarly

Same pattern in `dlc-core/src/swap.rs` — pass `GpuProvider`, add `with_memory_pattern(false)`, configure DirectML EP.

### Step 6: Update main.rs to pass EP config

```rust
let provider = GpuProvider::Auto;
let detector = FaceDetector::new(&det_path, &provider)?;
let swapper = FaceSwapper::new(&models_dir, &provider)?;
```

### Step 7: Update build script

In `scripts/build-rust-sidecar-win.ps1`:
```powershell
# Step 0: Ensure DirectML DLLs are present
pwsh scripts/setup-directml-dlls.ps1

# Step 1: Build with DirectML
$env:ORT_LIB_PATH = "$RepoRoot/core/rust-engine/ort-dml-libs"
cargo build --release -p dlc-server --features dlc-capture/opencv

# Step 2: Copy DLLs alongside sidecar
Copy-Item "$RepoRoot/core/rust-engine/ort-dml-libs/*.dll" $BinariesDir
```

### Step 8: Copy DLLs to install directory

Add ORT DirectML DLLs to the Tauri bundle alongside OpenCV DLLs.

## Todo

- [ ] Create `scripts/setup-directml-dlls.ps1`
- [ ] Update workspace `ort` dep: remove `download-binaries`, add `directml`
- [ ] Create `.cargo/config.toml` with `ORT_LIB_PATH`
- [ ] Add `GpuProvider` enum to `dlc-core/src/lib.rs`
- [ ] Update `FaceDetector::new()` with EP config
- [ ] Update `FaceSwapper::new()` with EP config
- [ ] Update `main.rs` to pass `GpuProvider::Auto`
- [ ] Update `build-rust-sidecar-win.ps1`
- [ ] Copy ORT DLLs to install dir
- [ ] Test: verify DirectML EP is selected in logs
- [ ] Test: measure FPS improvement vs CPU baseline
- [ ] Add `/health` endpoint field showing active EP

## Success Criteria

- Server logs show `DirectMLExecutionProvider` selected
- Face swap pipeline runs at 7+ FPS on Radeon 890M
- Graceful fallback to CPU if DirectML unavailable
- No additional user configuration required

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| NuGet DLL version mismatch with ort crate | Medium | High | Pin exact NuGet version matching ort's ORT version |
| DirectML silent fallback to CPU | Medium | Medium | Log EP selection, add `/health` field |
| Session init takes 5s+ on cold start | Low | Low | Acceptable for startup; sessions reused after |

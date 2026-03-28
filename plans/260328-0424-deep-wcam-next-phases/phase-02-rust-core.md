# Phase 2: Rust Core Rewrite (Approach A)

**Workstream:** WS2
**Owner:** Team 2 Leader
**Timeline:** Weeks 1-6 (parallel with WS3)
**Agents:** 5 CCS-delegated

---

## Objective

Rewrite the Python inference pipeline in Rust using `ort` crate v2.0. Produce a standalone binary that can replace the Python backend in the Tauri app or run independently as a CLI tool.

## Target Specifications

| Metric | Target | Python Baseline |
|--------|--------|-----------------|
| Binary size | < 80MB (excl. models) | N/A (Python install) |
| Startup time | < 0.3s | ~2s |
| Frame latency | <= 33ms (30fps) | ~35-40ms |
| Memory usage | < 500MB (1080p) | ~800MB |
| GPU support | CUDA, DirectML, CoreML | Same |

---

## Module Mapping: Python to Rust

```
Python                      Rust Crate / Module
──────────────────────────  ──────────────────────────────
face_analyser.py            src/face/analyser.rs (ort + SCRFD)
face_swapper.py             src/face/swapper.rs (ort + inswapper)
face_enhancer.py            src/face/enhancer.rs (ort + GFPGAN)
face_masking.py             src/face/masking.rs (ndarray ops)
core.py                     src/pipeline.rs (tokio async)
video_capture.py            src/capture/camera.rs (nokhwa)
virtual_camera.py           src/capture/virtual_cam.rs (platform FFI)
gpu_processing.py           src/gpu.rs (ort provider selection)
globals.py                  src/state.rs (RwLock<AppState>)
utilities.py                src/util.rs (ffmpeg subprocess)
ui.py                       NOT ported (Tauri frontend)
```

## Cargo.toml Dependencies

```toml
[dependencies]
ort = { version = "2.0", features = ["load-dynamic", "cuda"] }
ndarray = "0.16"
image = "0.25"
kornia = "0.1"           # Image preprocessing (faster than image crate)
nokhwa = "0.10"          # Cross-platform camera
tokio = { version = "1", features = ["full"] }
axum = "0.7"             # HTTP API (compatible with Tauri sidecar pattern)
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive"] }
tracing = "0.1"
anyhow = "1"

[target.'cfg(target_os = "linux")'.dependencies]
v4l2-sys-mit = "0.3"    # Virtual camera on Linux

[profile.release]
lto = true
strip = true
codegen-units = 1
```

---

## Steps

### Week 1: Project Scaffold + Model Validation Gate

**CCS Agents:**
- Claude (Agent 1): Architecture, Cargo.toml, module interfaces
- mmhs (Agent 2): Scaffold code, ort session loading

#### 1.1 Create Rust project structure
```
deep-live-cam-rs/
├── Cargo.toml (workspace)
├── crates/
│   ├── dlc-core/        # Face processing pipeline
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── face/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── analyser.rs
│   │   │   │   ├── swapper.rs
│   │   │   │   ├── enhancer.rs
│   │   │   │   └── masking.rs
│   │   │   ├── pipeline.rs
│   │   │   ├── state.rs
│   │   │   └── gpu.rs
│   │   └── Cargo.toml
│   ├── dlc-capture/     # Camera + virtual camera
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── camera.rs
│   │   │   └── virtual_cam.rs
│   │   └── Cargo.toml
│   └── dlc-cli/         # CLI binary
│       ├── src/main.rs
│       └── Cargo.toml
└── models/              # Symlink to shared models dir
```

**Delegation:** Claude designs interfaces (trait definitions), mmhs creates files.

#### 1.2 ort v2.0 Validation Gate (CRITICAL)

Load each ONNX model and run dummy inference:
```rust
// Validate inswapper_128.onnx loads and runs
let session = Session::builder()?
    .with_execution_providers([CUDAExecutionProvider::default().build()])?
    .commit_from_file("models/inswapper_128.onnx")?;

let input_shape = session.inputs[0].input_type.tensor_type().unwrap().dimensions;
println!("Input shape: {:?}", input_shape);  // Expected: [1, 3, 128, 128]
```

Models to validate:
- `inswapper_128.onnx` (FP32, face swap)
- `buffalo_l` SCRFD detector (face detection)
- `gfpgan-1024.onnx` (face enhancement)

**Gate:** If any model fails to load in ort, investigate opset version compatibility. Fallback: use `ort` with CPU provider or downgrade model opset.

---

### Weeks 2-3: Face Detection + Face Swap

**CCS Agents:**
- mmhs (Agent 2): SCRFD face detection
- mmhs (Agent 3): Inswapper face swap
- mm (Agent 5): Unit tests

#### 2.1 Face Detection (SCRFD via ort)

Port `face_analyser.py` logic:
1. Preprocess input frame: resize to 640x640, normalize to [0,1], CHW layout
2. Run SCRFD ONNX model
3. Post-process: decode bounding boxes, NMS, extract landmarks (5 keypoints)
4. Return `Vec<DetectedFace>` with bbox, landmarks, confidence

```rust
pub struct DetectedFace {
    pub bbox: [f32; 4],           // x1, y1, x2, y2
    pub landmarks: [[f32; 2]; 5], // 5 keypoints
    pub confidence: f32,
    pub embedding: Option<Array1<f32>>,
}

pub trait FaceAnalyser {
    fn detect(&self, frame: &Array3<u8>) -> Result<Vec<DetectedFace>>;
    fn get_one_face(&self, frame: &Array3<u8>) -> Result<Option<DetectedFace>>;
}
```

**Delegation:** mmhs implements from Python reference; mm writes tests with known test images.

#### 2.2 Face Swap (Inswapper via ort)

Port `face_swapper.py` logic:
1. Extract face embedding from source face (ArcFace model)
2. Align target face using landmark affine transform
3. Run inswapper_128 model: input = (aligned_face, embedding) → output = swapped_face
4. Inverse affine transform to place swapped face back
5. Poisson blending (seamless clone) at face boundary

```rust
pub trait FaceSwapper {
    fn swap(
        &self,
        source_face: &DetectedFace,
        target_face: &DetectedFace,
        frame: &mut Array3<u8>,
    ) -> Result<()>;
}
```

**Critical preprocessing:** Affine transform using landmark alignment. Python uses `cv2.getAffineTransform()` — Rust equivalent: manual 2x3 matrix computation or `kornia-rs` warp.

---

### Week 4: Face Enhancement + Mouth Masking

**CCS Agents:**
- mmhs (Agent 4): GFPGAN enhancer + mouth mask
- mm (Agent 5): Integration tests

#### 4.1 Face Enhancer (GFPGAN/GPEN)

Port `face_enhancer.py`:
1. Crop face region from frame
2. Resize to model input (1024x1024 for GFPGAN)
3. Run ONNX model
4. Resize output back, blend into frame

#### 4.2 Mouth Masking

Port `face_masking.py`:
1. Extract mouth region using face landmarks
2. Create binary mask for mouth area
3. Preserve original mouth pixels during face swap
4. Blend mask edges for smooth transition

---

### Week 5: Async Pipeline + Camera Capture

**CCS Agents:**
- Claude (Agent 1): Pipeline architecture (tokio channels)
- mmhs (Agent 2): nokhwa camera integration
- mmhs (Agent 4): Virtual camera output

#### 5.1 Async Processing Pipeline

```rust
// Pipeline: capture -> detect -> swap -> enhance -> output
async fn run_pipeline(config: PipelineConfig) -> Result<()> {
    let (frame_tx, frame_rx) = tokio::sync::mpsc::channel(4);
    let (output_tx, output_rx) = tokio::sync::mpsc::channel(4);

    // Capture task
    tokio::spawn(async move { capture_loop(frame_tx).await });

    // Processing task (GPU-bound, runs on blocking thread)
    tokio::spawn(async move {
        while let Some(frame) = frame_rx.recv().await {
            let result = tokio::task::spawn_blocking(move || {
                process_frame(frame, &pipeline)
            }).await?;
            output_tx.send(result).await?;
        }
    });

    // Output task
    tokio::spawn(async move { output_loop(output_rx).await });
}
```

#### 5.2 Camera Capture (nokhwa)

Cross-platform webcam access:
```rust
use nokhwa::Camera;
let camera = Camera::new(0, RequestedFormat::new::<RgbFormat>(
    RequestedFormatType::AbsoluteHighestFrameRate
))?;
```

#### 5.3 Virtual Camera Output

- **Linux:** v4l2loopback write via ioctl
- **Windows:** DirectShow virtual camera (FFI)
- **macOS:** CoreMediaIO (defer to Tauri app for now)

---

### Week 6: Integration + Benchmarks + Binary Packaging

**CCS Agents:**
- mm (Agent 5): Benchmarks, CI pipeline
- mmhs (Agent 3): HTTP API server (axum) for Tauri compatibility
- Claude (Agent 1): Final review, performance tuning

#### 6.1 HTTP API (Tauri Sidecar Compatibility)

Implement same API contract as Python FastAPI server:
```
GET  /health              → 200 OK
POST /swap/image          → multipart (source, target) → JPEG result
WS   /swap/live           → binary WebSocket (JPEG frames)
POST /config              → JSON (provider, model, options)
```

#### 6.2 Benchmarks

Compare Rust vs Python on V100:
- Single image swap latency (ms)
- 30fps live processing (sustained)
- Memory usage at 720p vs 1080p
- Startup time (cold + warm)

#### 6.3 Binary Packaging

```bash
cargo build --release
# Output: target/release/dlc-cli (~30-80MB)
# Bundle with: models/ + onnxruntime.so
cargo dist build  # Create platform installers
```

---

## Dependencies

| This Step | Depends On | Blocks |
|:-:|---|---|
| Week 1 (scaffold) | WS1 complete (stable Python baseline) | Week 2 |
| Week 1.2 (ort gate) | Models available | All subsequent weeks |
| Weeks 2-3 (detect+swap) | Week 1 | Week 4 |
| Week 4 (enhance+mask) | Weeks 2-3 | Week 5 |
| Week 5 (pipeline) | Week 4 | Week 6 |
| Week 6 (integration) | Week 5 + WS3 API contract | Ship |

## Success Criteria

- [ ] All ONNX models load and run in ort v2.0 on V100 (CUDA provider)
- [ ] Face detection matches Python output on test images (bbox IoU > 0.95)
- [ ] Face swap produces visually correct output (manual QA)
- [ ] 30fps sustained processing at 720p on V100
- [ ] Binary < 80MB (excluding models)
- [ ] HTTP API passes same integration tests as Python FastAPI server
- [ ] Startup time < 0.3s

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|:-:|:-:|---|
| ort v2.0 can't load inswapper model | Medium | Critical | Week 1 gate; fallback to PyO3 wrapper for swap module only |
| SCRFD post-processing differs from InsightFace Python | Medium | High | Compare outputs numerically; accept 95% IoU |
| kornia-rs missing needed transforms | Low | Medium | Fall back to image crate + manual affine |
| nokhwa camera issues on Linux | Low | Low | Fall back to OpenCV Rust bindings |
| Poisson blending hard to port | Medium | Medium | Use simpler alpha blending initially; iterate |
| Week 6 too tight for integration | Medium | High | HTTP API is optional for CLI; defer to Week 7 if needed |

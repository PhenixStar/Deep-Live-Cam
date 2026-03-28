# Phase 3: Rust Core Migration (Approach B — Sequential)

**Workstream:** WS2 (runs as Phase 3 in sequential pipeline)
**Owner:** Team 2 Leader (primary) + Team 1 Leader (testing)
**Timeline:** Weeks 5-8 (after Tauri v1.0 ships)
**Agents:** 5 CCS-delegated (Team 2) + 5 (Team 1, reassigned to testing)

---

## Objective

Incrementally replace the Python sidecar in the Tauri app with a Rust binary. The Rust binary must implement the same HTTP API contract defined in Week 2 (shared with Python FastAPI). Result: Tauri app v2.0 with Rust backend, same frontend, smaller binary, faster startup.

## Differences from Approach A (phase-02-rust-core.md)

| Aspect | Approach A (6 weeks) | Approach B (4 weeks) |
|--------|---------------------|---------------------|
| Timeline | Weeks 1-6 | Weeks 5-8 |
| Starting point | Greenfield | HTTP API contract + test suite from v1.0 |
| Validation | Manual QA | Automated: same integration tests as Python |
| Risk | Higher (no reference impl) | Lower (Python impl = reference oracle) |
| Frontend | May need own UI | Tauri frontend unchanged |
| Agent count | 5 | 10 (Team 1 reassigned) |

**Key advantage of Approach B:** The Python FastAPI server is the reference implementation. Every Rust endpoint can be validated by comparing output to Python output on same inputs. This is a massive de-risking strategy.

---

## Week 5: Scaffold + ort Validation + Face Detection

**CCS Agents:**
- Claude (Agent 1): Architecture, Cargo.toml, trait definitions
- mmhs (Agent 2): ort model loading + SCRFD face detection
- mmhs (Agent 3): HTTP API skeleton (axum, matching FastAPI contract)
- mm (Team 1, Agent 3): Comparison test harness (Rust vs Python output)

### 5.1 Project scaffold

Same structure as Approach A (see phase-02-rust-core.md, Week 1, Step 1.1).

Workspace with 3 crates:
- `dlc-core`: Face processing pipeline
- `dlc-capture`: Camera + virtual camera
- `dlc-server`: axum HTTP/WS server (replaces FastAPI)

### 5.2 ort v2.0 Validation Gate

**CRITICAL:** Load all 3 models in ort on V100 with CUDA provider:
- `inswapper_128.onnx` (FP32)
- SCRFD detector (from buffalo_l)
- `gfpgan-1024.onnx`

**Gate rule:** If any model fails, stop and diagnose. Options:
1. Downgrade model opset version
2. Use CPU provider for that specific model
3. Fall back to PyO3 bridge for that module only

### 5.3 axum HTTP API skeleton

Implement all endpoints from the API contract (stubs returning 501):

```rust
use axum::{Router, routing::{get, post}, extract::ws::WebSocket};

fn app() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/swap/image", post(swap_image))
        .route("/swap/live", get(swap_live_ws))
        .route("/config", post(update_config))
        .route("/cameras", get(list_cameras))
        .route("/cameras/select", post(select_camera))
}
```

### 5.4 Comparison test harness

```bash
# Run same request against Python and Rust servers
# Compare outputs (JPEG similarity, JSON equality)
python tests/compare_backends.py \
  --python-url http://localhost:8008 \
  --rust-url http://localhost:8009 \
  --test-image test_assets/source.jpg \
  --threshold 0.95  # SSIM threshold for image comparison
```

**Delegation:** mm builds the comparison harness. Each Rust endpoint is validated against Python oracle.

### 5.5 SCRFD face detection

Port face detection (same as Approach A, Week 2-3, Step 2.1):
1. Preprocess: resize 640x640, normalize, CHW layout
2. Run SCRFD model via ort
3. Post-process: decode boxes, NMS, extract landmarks
4. Return `Vec<DetectedFace>`

**Validation:** Run detection on 10 test images, compare bounding boxes to Python output. IoU threshold > 0.95.

---

## Week 6: Face Swap + Embedding Extraction

**CCS Agents:**
- mmhs (Agent 2): ArcFace embedding extraction
- mmhs (Agent 3): Inswapper face swap
- mmhs (Agent 4): Affine transform + blending
- mm (Team 1, Agent 3): Comparison tests per module

### 6.1 ArcFace embedding

Extract 512-dim face embedding from detected face:
1. Align face using 5 landmark points (affine transform to canonical pose)
2. Resize aligned face to 112x112
3. Run ArcFace model via ort
4. L2-normalize output embedding

### 6.2 Inswapper face swap

1. Input: aligned target face (128x128) + source embedding (512-dim)
2. Run inswapper_128 model
3. Output: swapped face (128x128)

### 6.3 Face placement (inverse affine + blending)

1. Compute inverse affine transform
2. Warp swapped face back to original frame coordinates
3. Create seamless boundary mask
4. Alpha blend (or Poisson blend if performance allows)

### 6.4 Wire up `/swap/image` endpoint

Replace stub with real implementation:
```rust
async fn swap_image(multipart: Multipart) -> impl IntoResponse {
    let (source, target) = extract_images(multipart).await?;
    let source_face = analyser.get_one_face(&source)?;
    let target_face = analyser.get_one_face(&target)?;
    let mut result = target.clone();
    swapper.swap(&source_face, &target_face, &mut result)?;
    encode_jpeg(&result)
}
```

**Validation:** Compare JPEG output of `/swap/image` between Python and Rust. SSIM > 0.90 (blending differences acceptable).

---

## Week 7: Enhancers + Pipeline + Live Streaming

**CCS Agents:**
- mmhs (Agent 4): GFPGAN enhancer + mouth masking
- Claude (Agent 1): Async pipeline design (tokio channels)
- mmhs (Agent 2): WebSocket binary streaming
- mm (Team 1, Agent 3): Live streaming comparison tests

### 7.1 GFPGAN face enhancer

1. Crop face region, resize to 1024x1024
2. Run GFPGAN model via ort
3. Resize output, blend back into frame
4. Optional: GPEN-256/512 alternatives

### 7.2 Mouth masking

1. Extract mouth landmarks (subset of 5 keypoints)
2. Create convex hull mask around mouth region
3. During swap: preserve original mouth pixels
4. Feather mask edges for smooth blending

### 7.3 Async pipeline (tokio)

```rust
async fn run_live_pipeline(ws: WebSocket, config: Arc<RwLock<Config>>) {
    let (mut ws_tx, mut ws_rx) = ws.split();
    let (frame_tx, mut frame_rx) = mpsc::channel(2); // small buffer, skip old frames

    // Camera capture task
    tokio::spawn(capture_camera_loop(config.clone(), frame_tx));

    // Processing + sending task
    while let Some(frame) = frame_rx.recv().await {
        let config = config.read().await.clone();
        let result = tokio::task::spawn_blocking(move || {
            process_frame(&frame, &config)
        }).await?;

        let jpeg = encode_jpeg_bytes(&result, config.jpeg_quality);
        if ws_tx.send(Message::Binary(jpeg)).await.is_err() {
            break;
        }
    }
}
```

### 7.4 Camera capture

Use `nokhwa` for cross-platform camera access. Wire up `/cameras` and `/cameras/select` endpoints.

### 7.5 Wire up `/swap/live` WebSocket

Replace stub with real implementation. Test at 30fps on V100.

**Validation:** Side-by-side live stream comparison (Python vs Rust). Visual quality check + FPS measurement.

---

## Week 8: Integration + Benchmark + Ship v2.0

**CCS Agents:**
- mm (Team 1, Agent 3): Full integration test suite
- mm (Team 1, Agent 5): Benchmark suite
- mmhs (Agent 3): Replace sidecar binary in Tauri app
- Claude (Agent 1): Performance tuning, final review
- mmhs (Team 3, Agent 4): Rebuild platform installers with Rust binary

### 8.1 Replace Python sidecar with Rust binary

Update `tauri.conf.json`:
```json
{
  "bundle": {
    "externalBin": [
      "bin/dlc-server-x86_64-unknown-linux-gnu",
      "bin/dlc-server-x86_64-pc-windows-msvc"
    ]
  }
}
```

Build Rust binary:
```bash
cargo build --release -p dlc-server
cp target/release/dlc-server src-tauri/bin/dlc-server-x86_64-unknown-linux-gnu
```

### 8.2 Benchmarks (Rust vs Python)

| Metric | Python Baseline | Rust Target | Measured |
|--------|:-:|:-:|:-:|
| Startup time | ~2s | < 0.3s | TBD |
| Image swap latency | ~40ms | < 33ms | TBD |
| Live FPS (720p, V100) | ~28fps | >= 30fps | TBD |
| Memory usage (720p) | ~800MB | < 500MB | TBD |
| Binary size | ~200MB (PyInstaller) | < 80MB | TBD |

### 8.3 Integration test suite

Run full test suite against Rust server:
```bash
# All endpoints
pytest tests/integration/ --server-url http://localhost:8009

# Comparison against Python oracle
python tests/compare_backends.py --full-suite

# Stress test: 1000 image swaps
python tests/stress_test.py --count 1000 --server http://localhost:8009
```

### 8.4 Rebuild platform installers

- Rebuild AppImage and MSI with Rust binary (replaces Python sidecar)
- Binary size should drop from ~300MB to ~120MB (no Python runtime)
- Same frontend, same Tauri shell

### 8.5 Ship v2.0

```bash
git tag v2.0.0
gh release create v2.0.0 \
  dist/deep-live-cam_2.0.0_amd64.AppImage \
  dist/deep-live-cam_2.0.0_amd64.deb \
  dist/deep-live-cam_2.0.0_x64.msi \
  --title "Deep-Live-Cam v2.0.0 - Rust Backend" \
  --notes "Replaced Python backend with Rust. Faster startup, smaller binary, same UI."
```

---

## Dependencies

| This Step | Depends On | Blocks |
|:-:|---|---|
| Week 5 | v1.0 shipped (Week 4), API contract | Week 6 |
| Week 5 (ort gate) | ONNX models available | All subsequent weeks |
| Week 6 | Week 5 (detection works) | Week 7 |
| Week 7 | Week 6 (swap works) | Week 8 |
| Week 8 | Week 7 (pipeline works) | Ship v2.0 |
| Comparison harness | Python server running | All validation steps |

## Success Criteria

- [ ] All ONNX models load in ort v2.0 on V100
- [ ] `/swap/image` output SSIM > 0.90 vs Python
- [ ] `/swap/live` sustains 30fps at 720p on V100
- [ ] All integration tests pass (same tests as Python server)
- [ ] Binary size < 80MB (excluding models)
- [ ] Startup time < 0.3s
- [ ] Tauri app v2.0 works with Rust binary as drop-in replacement
- [ ] Platform installers (AppImage, MSI) rebuild successfully

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|:-:|:-:|---|
| ort can't load inswapper model | Medium | Critical | Week 5 gate; fallback to PyO3 for swap module only |
| Face detection accuracy < Python | Medium | High | Compare numerically; accept 95% IoU; tune NMS params |
| Poisson blending too slow in Rust | Medium | Medium | Use alpha blending; optimize later |
| WebSocket throughput < Python | Low | Medium | tokio is faster than asyncio; unlikely bottleneck |
| 4 weeks too tight for full pipeline | High | Medium | Prioritize image swap; defer live streaming to Week 9 |
| Comparison harness gives false positives | Low | Medium | Manual visual QA on 10 samples per endpoint |

## Fallback Plan

If Week 5 ort validation gate fails:
1. **Partial Rust:** Port only non-ONNX modules (camera, pipeline, HTTP server) to Rust
2. **PyO3 bridge:** Keep Python ONNX inference via PyO3 embedded interpreter
3. **Result:** Hybrid binary (~100MB), faster startup, but still depends on Python for inference
4. **Timeline impact:** +1 week for PyO3 integration

If full Rust rewrite takes > 4 weeks:
1. Ship v2.0 with whatever modules are complete in Rust
2. Remaining modules use Python via PyO3 bridge
3. Complete migration in v2.1 (Week 9-10)

# Speed Optimization Brainstorm

**Date:** 2026-03-30
**Current:** 490ms/swap DirectML (Sweep), 825ms/swap NPU INT8, 10ms/swap V100 CUDA
**Target:** <100ms/swap (10+ FPS live) on Sweep AMD APU

---

## Top 10 Speed Wins (ranked by impact/effort)

### 1. Face Tracking — Skip Detection (BIGGEST WIN)
**Impact: 30-50% FPS boost | Effort: 4h**

Detect face every 5-10 frames. Track between using optical flow or KCF tracker.
- Detection is ~200ms of the 490ms pipeline on DirectML
- Tracking is <5ms per frame
- FaceFusion does this — key to their 30fps

```
Frame 1: DETECT (200ms) + SWAP (290ms) = 490ms
Frame 2: TRACK (5ms)    + SWAP (290ms) = 295ms  ← 40% faster
Frame 3: TRACK (5ms)    + SWAP (290ms) = 295ms
...
Frame 10: DETECT again
```

### 2. Source Embedding Caching (ALREADY DONE)
**Impact: ~35ms/frame saved | Status: SHIPPED**

Pre-compute source face embedding on upload. Skip re-detection per frame.

### 3. ReSwapper 256 Instead of Inswapper 128
**Impact: Better quality, similar speed | Effort: 2h**

ReSwapper 256 produces 256px output (vs 128px) eliminating the need for GFPGAN enhancement.
- Skip enhancer = save 200-300ms per frame
- Net result: better quality AND faster
- Model already in `core/models/reswapper_256.onnx` (554MB)

### 4. OnnxSlim Model Optimization
**Impact: ~5% inference speedup | Effort: 30min**

```bash
pip install onnxslim
onnxslim inswapper_128_fp16.onnx inswapper_128_fp16_slim.onnx
```
Removes redundant identity nodes, fuses operators. Free performance.

### 5. Resolution Downscaling Before Detection
**Impact: 2-4x detection speedup | Effort: 1h**

Current: detect on full 640x480 frame (resized to 640x640)
Optimization: detect on 320x320, scale bboxes back up
- SCRFD accuracy drops ~2% at 320x320 but detection is 4x faster
- Only affects detection, not swap quality

### 6. Async Pipeline (Double Buffering)
**Impact: Hides detection latency | Effort: 4h**

```
Thread A: Capture frame N → Detect → Queue
Thread B: Dequeue → Swap → Encode → Send
```
Detection of frame N+1 overlaps with swap of frame N.
On Sweep with 12 cores: plenty of parallelism.

### 7. FP16 Compute on DirectML
**Impact: ~30% faster on Radeon 890M | Effort: 1h**

DirectML supports FP16 natively. The `inswapper_128_fp16.onnx` already exists.
Ensure ort Level3 optimization is applying FP16 graph rewrites (already enabled).
Verify with: check if ort logs show "FP16 transform applied".

### 8. TurboJPEG Instead of Image Crate
**Impact: 5-10ms per frame | Effort: 2h**

The `image` crate's JPEG encoder is slow (~15ms for 640x480).
TurboJPEG (libjpeg-turbo) does it in ~3ms. Use the `turbojpeg` Rust crate.

### 9. Frame Skipping (Adaptive)
**Impact: Perceived smoothness | Effort: 1h**

If swap takes >33ms, skip the next frame capture. Send most recent processed frame.
Better than buffering old frames and adding latency.

### 10. Batch Face Detection + Swap
**Impact: 20% for multi-face | Effort: 3h**

SCRFD supports batch inference. Detect all faces in one ort::run instead of sequential.
Only matters for multi-face scenarios.

---

## Model Alternatives

| Model | Resolution | Speed (GPU) | Quality | Worth switching? |
|-------|-----------|-------------|---------|:---:|
| **inswapper_128** (current) | 128px | 30-50ms | 6/10 | Baseline |
| **ReSwapper 256** | 256px | 70-90ms | 8/10 | **YES — skip enhancer** |
| **HyperSwap 1a** | 256px | 60-80ms | 7.5/10 | YES — handles occlusions |
| **HyperSwap 1c** | 256px | 85-100ms | 9/10 | Quality mode |
| **SimSwap++** | 512px | 55-70ms | 8.5/10 | Research — may not have ONNX |
| **FastSwap** | 256px | <10ms | 7/10 | **Best for speed** |

**Recommendation:** Add ReSwapper 256 as "quality mode" (skip enhancer), keep inswapper 128 as "speed mode".

---

## From Upstream & Forks

| Source | Technique | FPS Gain |
|--------|-----------|----------|
| FaceFusion | TensorRT EP | +20-40% (NVIDIA only) |
| FaceFusion | Face tracking (skip N frames) | +30-50% |
| FaceFusion | OnnxSlim model optimization | +5% |
| PR #1710 | DirectML serialization lock | Prevents crashes |
| PR #1677 | CoreML warmup + det_size switching | macOS specific |
| iRoopDeepFaceCam fork | Virtual camera + low-latency WS | Streaming focused |

---

## Implementation Priority

| # | Optimization | Est. FPS Gain | Effort | Do It? |
|---|-------------|:---:|:---:|:---:|
| 1 | Face tracking (skip N frames) | +50% | 4h | **YES — #1 priority** |
| 2 | ReSwapper 256 (skip enhancer) | +40% | 2h | **YES** |
| 3 | OnnxSlim on all models | +5% | 30min | **YES — free** |
| 4 | Async pipeline (double buffer) | +30% | 4h | **YES** |
| 5 | Detection downscale (320x320) | +15% | 1h | **YES** |
| 6 | TurboJPEG encoding | +5% | 2h | Maybe later |
| 7 | Frame skipping | +10% perceived | 1h | Easy win |

Combined effect of #1-5: **490ms → ~150ms (6-7 FPS → 20+ FPS on Sweep)**

---

## Unresolved Questions
1. Is SimSwap++ ONNX available? Need to check GitHub
2. Does FastSwap have a public ONNX model?
3. Can we run detection on NPU while swap runs on DirectML simultaneously?
4. What's the actual FP16 speedup on Radeon 890M DirectML?

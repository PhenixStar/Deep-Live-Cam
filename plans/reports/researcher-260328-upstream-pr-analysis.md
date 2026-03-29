# Upstream PR Analysis: Deep-Live-Cam DirectML & Safety Fixes

**Date:** 2026-03-28
**Focus:** PR #1710 (DirectML AMD optimization) and PR #1715 (null check fixes)
**Target:** Applicability to Rust implementation

---

## Executive Summary

Two upstream PRs address critical issues in the Python codebase:
1. **PR #1710** — DirectML AMD GPU optimization (serialization, detection throttling)
2. **PR #1715** — Null check safety for image loading

Our Rust implementation **already addresses core safety concerns** via type safety and error handling. However, **AMD DirectML concurrency patterns merit investigation** as we currently use std::sync::Mutex without explicit serialization locks.

---

## PR #1710: AMD DirectML Optimization

### What It Does

Fixes crashes on AMD GPUs (RX 5000/6000/7000 series) running DirectML by:
- Adding a **global threading lock** (`dml_lock`) to serialize all DML inference
- Pre-loading face analyzer + swapper models before GUI startup
- Disabling detection thread; detection runs **inline** with processing
- **Throttling detection** (every 3rd frame) to maximize GPU throughput
- Adjusting `OMP_NUM_THREADS` from 1 → 6

### Performance Results (AMD RX 6700, 10GB)
```
No enhancer: ~24 FPS
GPEN-256:   ~11 FPS
GFPGAN:     ~8 FPS
GPU: 60-100% utilization
```

### Python Implementation Details

**Files Changed:**
- `modules/globals.py` — global lock
- `modules/face_analyser.py` — model pre-load
- `modules/processors/frame/face_swapper.py` — serialized inference
- `modules/ui.py` — detection pipeline changes
- `run-dml.bat` — new Windows DirectML helper

**Key Pattern:**
```python
with dml_lock:  # Global lock around all DML inference
    outputs = session.run(...)
```

### Applicability to Rust Implementation

**Current State:**
- We use `std::sync::Mutex<Option<FaceDetector>>` per model (lib.rs:80-84)
- Session runs inline without explicit lock serialization
- Detection runs on demand (no separate thread to disable)
- No OMP_NUM_THREADS environment variable set

**Recommendation: MEDIUM PRIORITY**

**Why:**
- Our Mutex-per-model is coarse-grained but NOT serializing all DML operations globally
- AMD DirectML may have issues with concurrent session access even when wrapped in Mutex
- Python's issue specifically mentions concurrent *session creation/initialization*, not just inference

**Actions to Implement:**

1. **Add global DirectML serialization lock** (only on DirectML provider):
   ```rust
   // In dlc_core::lib.rs or dlc_server::main.rs
   static DML_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

   // In load_session() or before each session.run():
   if matches!(self, GpuProvider::DirectML { .. }) {
       let _guard = DML_LOCK.lock().unwrap();
       // ... session.run() ...
   }
   ```

2. **Set OMP_NUM_THREADS=6 in main.rs startup**:
   ```rust
   // In main() before creating models
   std::env::set_var("OMP_NUM_THREADS", "6");
   ```

3. **Document AMD detection throttling pattern** (if needed):
   - Currently no separate detection thread exists; already inline
   - If future architecture adds threading, apply 3-frame throttle on detection-only runs

4. **Test on AMD RX 5000/6000/7000 series** if possible

---

## PR #1715: Null Check Safety Fixes

### What It Does

Fixes `AttributeError: 'NoneType' object has no attribute 'shape'` crash (issue #1640).

**Root Cause:**
- `cv2.imread()` returns `None` for unreadable/invalid image files
- Code attempted `.shape` access on `None`, causing crash

**Fixes in `modules/face_analyser.py`:**
1. `get_unique_faces_from_target_image()` — validate image loaded before detection
2. `get_unique_faces_from_target_video()` — skip frames where `cv2.imread()` returns None
3. Handle face detection returning `None` gracefully

### Applicability to Rust Implementation

**Current State:**
- We use `image::load_from_memory()` in router.rs:291, which returns `Result<T>`
- Rust's type system prevents None-on-None operations (compiler enforces Option/Result)
- Error handling in upload_source() already validates image load (line 291-296):
  ```rust
  if let Err(e) = image::load_from_memory(&bytes) {
      return (StatusCode::UNPROCESSABLE_ENTITY, ...)
  }
  ```

**Recommendation: NO ACTION REQUIRED**

**Why:**
- Our Rust code **cannot** have Python's None-access bug
- `image::load_from_memory()` returns `Result`, forcing explicit error handling
- `decode_to_bgr_frame()` (line 312) propagates error with `?` operator
- Face detection methods return `Result<Vec<DetectedFace>>`, not Option

**Verification:**
- upload_source() validates before storing (line 291)
- swap_image() calls decode_to_bgr_frame() with propagating error (line 386)
- detect.rs enforces face detection validation (line 69-84)

**Strength:** Our type system is stricter than Python's None semantics.

---

## Recent PRs (March 2026)

No specific March 2026 merged PRs found in search results. Latest releases:
- **2.7-beta** (March 11, 2026) — multi-GPU and performance improvements
- Earlier stable: 2.6d (2024)

---

## Actionable Checklist

| Item | Priority | Status | Owner |
|------|----------|--------|-------|
| Add global DML serialization lock | MEDIUM | TODO | Backend |
| Set OMP_NUM_THREADS=6 for AMD | MEDIUM | TODO | Backend |
| Document DirectML caveats for AMD users | LOW | TODO | Docs |
| Test on AMD RX 6700+ if available | MEDIUM | TODO | QA |
| Null safety (PR #1715) | NONE | RESOLVED | N/A |

---

## Summary

- **PR #1715** (null safety): Already handled by Rust's type system — no action needed
- **PR #1710** (DirectML AMD): Two concrete improvements needed for AMD user experience:
  1. Global DML lock serialization
  2. OMP_NUM_THREADS environment variable

Neither is a blocker; both improve AMD DirectML stability and performance on concurrent workloads.

---

## Unresolved Questions

1. Have users reported AMD DirectML crashes in the Rust implementation yet?
2. Is OMP_NUM_THREADS=6 optimal for all AMD VRAM sizes, or should it be configurable?
3. Should detection throttling be added to future camera thread architectures?

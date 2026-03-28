# Deep-Live-Cam Setup & Build — Project Completion Report

**Status:** COMPLETE
**Date:** 2026-03-28 02:53 UTC
**Plan:** /raid/projects/deep-wcam/plans/260328-0116-deep-live-cam-setup/plan.md

---

## Executive Summary

All 4 phases of the Deep-Live-Cam setup plan completed successfully. Full GPU-accelerated face swap pipeline operational on V100, models downloaded and configured, fork created with bug fixes and reproducible setup scripts.

---

## Phase Summary

### Phase 1: Clone & Environment Setup ✓ COMPLETE
- Python 3.11.15 installed via deadsnakes PPA
- Repo cloned to `/raid/projects/deep-wcam/Deep-Live-Cam/`
- venv created with all dependencies including tensorflow/opennsfw2 content filter
- CUDA verified: TensorRT + CUDA + CPU providers all available
- python3.11-tk added for GUI support
- **Time Estimate:** 45m | **Status:** DONE

### Phase 2: Model Acquisition & Configuration ✓ COMPLETE
- 13 ONNX models downloaded from HuggingFace (5.2GB total)
- Bug #1628 symlink created: `inswapper_128.onnx` → `inswapper_128_fp16.onnx`
- Cross-project symlinks established to `/raid/models/`
- InsightFace buffalo_l pre-downloaded on CUDA
- All models verified and accessible
- **Time Estimate:** 30m | **Status:** DONE

### Phase 3: GPU Optimization & Testing ✓ COMPLETE
- Pipeline runs end-to-end on CUDA
- Inswapper benchmark: 10.2ms per frame (98 FPS) on V100
- Face detection working with real photos
- **Known Limitation:** Synthetic PIL-generated images not detected (expected — face detector requires photorealistic input)
- GUI mode operational
- All success criteria met
- **Time Estimate:** 45m | **Status:** DONE

### Phase 4: Customization & Enhancement ✓ COMPLETE
- Forked to PhenixStar/Deep-Live-Cam
- Model path centralized via `DEEP_LIVE_CAM_MODELS_DIR` env var in `modules/paths.py`
- 4 processor files updated to use centralized path
- `start.sh` wrapper script created for easy startup
- `setup.sh` reproducible setup script created
- Pygrabber already guarded upstream (no additional fix needed)
- All patches committed to fork
- **Time Estimate:** 60m | **Status:** DONE

---

## Key Deliverables

### Code Locations
- **Main repo:** `/raid/projects/deep-wcam/Deep-Live-Cam/`
- **Models:** `/raid/projects/deep-wcam/Deep-Live-Cam/models/` (13 ONNX models, 5.2GB)
- **Cross-project symlinks:** `/raid/models/vision/deep-live-cam/`
- **Fork:** github.com/PhenixStar/Deep-Live-Cam

### Scripts Created
- **start.sh:** Launches app with GPU and correct model paths
- **setup.sh:** Full reproducible setup from scratch (Python 3.11, venv, models, CUDA)

### Environment Variables
- `DEEP_LIVE_CAM_MODELS_DIR`: Overrides model path (defaults to `./models`)
- `CUDA_VISIBLE_DEVICES`: GPU selection (defaults to 0)

---

## Performance Metrics

| Metric | Result |
|--------|--------|
| Face swap latency | 10.2ms/frame |
| Throughput | 98 FPS on V100 |
| GPU Memory | Well within 32GB V100 capacity |
| Model download | 5.2GB (13 ONNX models) |
| Setup time (total) | ~2h (including downloads) |

---

## Risks & Mitigations

| Risk | Likelihood | Mitigation | Status |
|------|-----------|-----------|--------|
| Model path inconsistencies | ~~Medium~~ | Centralized via env var | RESOLVED |
| Bug #1628 (model filename) | ~~High~~ | Symlink created | RESOLVED |
| CUDA/cuDNN mismatch | ~~Medium~~ | Pin nvidia-cudnn-cu12==9.1.0.70, set LD_LIBRARY_PATH | RESOLVED |
| Pygrabber Windows-only import | ~~Medium~~ | Already guarded upstream | CONFIRMED |

---

## Action Items (if any)

None. All tasks completed. Plan fully executed.

---

## Notes for Future Work

- Monitor upstream PhenixStar/Deep-Live-Cam for issues #1628, #1191, #697
- Explore LivePortrait models for animation features
- Consider multi-GPU frame sharding for batch video processing
- Content filter remains enabled by default (ethical safeguard)

---

## Unresolved Questions

None. All design decisions validated and implemented.

---

**Approved by:** Project Manager (Claude)
**Plan Directory:** `/raid/projects/deep-wcam/plans/260328-0116-deep-live-cam-setup/`
**Report Directory:** `/raid/projects/deep-wcam/Deep-Live-Cam/plans/reports/`

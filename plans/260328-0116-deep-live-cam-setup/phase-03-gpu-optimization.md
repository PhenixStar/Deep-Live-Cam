---
phase: 3
title: "GPU Optimization & Testing"
status: complete
priority: P1
effort: 45m
executor: "team:gpu-test + ccs mm + Claude"
completed: 2026-03-28
---

# Phase 3: GPU Optimization & Testing

- **Parent Plan:** [plan.md](plan.md)
- **Dependencies:** [Phase 1](phase-01-clone-env-setup.md) + [Phase 2](phase-02-model-setup.md) (both must complete)
- **Date:** 2026-03-28

## Overview

Run Deep-Live-Cam end-to-end. Test CLI face swap, video processing, and GUI mode. Benchmark on V100 32GB. Configure FP16 and optimal settings. Runs as `/team "gpu-test"` — routine tests via CCS mm, failure debugging via Claude.

## Key Insights

- V100 32GB + FP16 Tensor Cores — overkill for this workload, max settings safe
- CLI: `python run.py --execution-provider cuda`
- GUI: requires X11/display — test with `DISPLAY=:0` or X forwarding
- Multi-GPU: not natively supported; use `CUDA_VISIBLE_DEVICES` to pin
- All models downloaded (user chose full download) — test all enhancers
- Repo cloned to `/raid/projects/deep-wcam/Deep-Live-Cam/`

## Requirements

- Phase 1 + 2 complete (venv, all models)
- Test source face image + target image/video
- Display server for GUI mode (X11/Wayland)

## Related Code Files

- `run.py` — main entry point
- `modules/processors/frame/face_swapper.py` — swap logic
- `modules/processors/frame/face_enhancer.py` — GFPGAN/GPEN post-process
- `modules/globals.py` — runtime config
- `modules/core.py` — orchestrator

## Implementation Steps

### Step 1: Create test assets `[ccs mmhs]`

```bash
cd /raid/projects/deep-wcam/Deep-Live-Cam
mkdir -p test_assets
# Generate synthetic test face (thispersondoesnotexist or local)
wget -O test_assets/source.jpg "https://thispersondoesnotexist.com" 2>/dev/null || \
  echo "Place a face image at test_assets/source.jpg manually"
# Copy source as target for self-swap test
cp test_assets/source.jpg test_assets/target.jpg 2>/dev/null
```

### Step 2: Verify CUDA + imports `[ccs mm]`

```bash
cd /raid/projects/deep-wcam/Deep-Live-Cam
source .venv/bin/activate
python -c "
import onnxruntime as ort
print('ONNX providers:', ort.get_available_providers())
import torch; print('CUDA:', torch.cuda.is_available(), torch.cuda.get_device_name(0))
import insightface; print('insightface OK')
print('ALL IMPORTS OK')
"
```

### Step 3: CLI face swap test — single image `[ccs mm]`

```bash
source .venv/bin/activate
python run.py \
  --source test_assets/source.jpg \
  --target test_assets/target.jpg \
  --output test_assets/output.jpg \
  --execution-provider cuda \
  --frame-processor face_swapper
```

### Step 4: Face swap + enhancement `[ccs mm]`

```bash
python run.py \
  --source test_assets/source.jpg \
  --target test_assets/target.jpg \
  --output test_assets/output_enhanced.jpg \
  --execution-provider cuda \
  --frame-processor face_swapper face_enhancer
```

### Step 5: GUI mode test `[ccs mm]`

```bash
export DISPLAY=:0
python run.py --execution-provider cuda
```

> If headless: skip this step, note in results. Try X forwarding if SSH.

### Step 6: Benchmark with GPU monitoring `[ccs mm]`

```bash
# Monitor GPU in background
nvidia-smi dmon -s mu -d 1 > test_assets/gpu_log.txt &
GPU_PID=$!

# Timed face swap
time python run.py \
  --source test_assets/source.jpg \
  --target test_assets/target.jpg \
  --output test_assets/output_bench.jpg \
  --execution-provider cuda \
  --frame-processor face_swapper face_enhancer

kill $GPU_PID 2>/dev/null
cat test_assets/gpu_log.txt
```

### Step 7: Analyze results & debug failures `[Claude — needs reasoning]`

If any step fails, Claude analyzes error output, reads relevant source files, and determines fix. Common failures:
- NoneType: face not detected → ensure test image has clear frontal face
- Model not found: path issue → verify symlinks from Phase 2
- CUDA OOM: impossible on V100 32GB, but check if wrong GPU selected
- GUI crash: no display server → use CLI-only

## CCS Delegation Map

| Step | Profile | Reason |
|------|---------|--------|
| 1 | `mmhs` | Simple wget/mkdir |
| 2-6 | `mm` | Run commands, collect output |
| 7 | Claude | Debug reasoning if failures occur |

## Todo

- [x] Create/obtain test assets `[ccs mmhs]`
- [x] Verify CUDA + all imports `[ccs mm]`
- [x] CLI single image face swap `[ccs mm]`
- [x] Face swap + enhancement pipeline `[ccs mm]`
- [x] GUI mode test `[ccs mm]`
- [x] Benchmark with GPU monitoring `[ccs mm]`
- [x] Analyze results, debug if needed `[Claude]`

## Success Criteria

1. Single image face swap produces valid output
2. Enhancement pipeline (GFPGAN) produces visibly improved output
3. GPU utilization >50% during processing (CUDA not CPU fallback)
4. No NoneType or model-not-found errors
5. GUI launches (if display available)

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| NoneType face detection error | Medium | Use clear frontal face images |
| GUI crash on headless | High | Accept CLI-only if no display |
| Output file missing | Medium | Check temp dir permissions |
| CUDA OOM | Very Low | V100 32GB is massive overkill |

## Security Considerations

- Use synthetic/AI-generated faces for testing (not real people)
- Content filter (opennsfw2) enabled by default — may block some test images

## Completion Notes

Pipeline runs end-to-end on CUDA successfully. Inswapper benchmark: 10.2ms per frame (98 FPS) on V100. Important limitation: Face detection requires real photos — synthetic PIL-generated images are not detected by the model (expected behavior, not a bug).

## Next Steps

Proceed to Phase 4.

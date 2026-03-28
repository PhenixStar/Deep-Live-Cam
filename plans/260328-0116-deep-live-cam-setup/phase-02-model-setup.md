---
phase: 2
title: "Model Acquisition & Configuration"
status: complete
priority: P1
effort: 30m
executor: "team:model-download + ccs mmhs"
completed: 2026-03-28
---

# Phase 2: Model Acquisition & Configuration

- **Parent Plan:** [plan.md](plan.md)
- **Dependencies:** None (runs parallel with Phase 1 — models download independently)
- **Parallel with:** [Phase 1](phase-01-clone-env-setup.md)
- **Date:** 2026-03-28

## Overview

Download ALL models from HuggingFace repo (~5.76GB), create symlinks for bug #1628 workaround, symlink to /raid/models/ for cross-project reuse. Runs as `/team "model-download"` with CCS mmhs for fast downloads.

## Key Insights

- HuggingFace repo `hacksider/deep-live-cam` — download entire repo (user validated: all models upfront)
- Bug #1628: code references both `inswapper_128.onnx` and `inswapper_128_fp16.onnx` — symlink needed
- InsightFace auto-downloads `buffalo_l` (~200MB) to `~/.insightface/models/` on first run
- User authenticated as `phenixstar` on HuggingFace — no rate limits expected
- Clone path: `/raid/projects/deep-wcam/Deep-Live-Cam/models/`

## Requirements

- HuggingFace CLI (`huggingface-hub` pip package)
- Network access to huggingface.co
- ~6GB disk space for all models

## Related Code Files

- `modules/face_swapper.py` — loads inswapper model
- `modules/face_enhancer.py` — loads GFPGAN model
- `modules/globals.py` — model path constants

## Implementation Steps

### Step 1: Create models directory `[ccs mmhs]`

```bash
mkdir -p /raid/projects/deep-wcam/Deep-Live-Cam/models
mkdir -p /raid/models/vision/deep-live-cam
```

### Step 2: Install huggingface-hub (standalone, no venv needed) `[ccs mmhs]`

```bash
pip install --user huggingface-hub 2>/dev/null || pip3 install --user huggingface-hub
```

### Step 3: Download ALL models from HuggingFace `[ccs mmhs]`

```bash
huggingface-cli download hacksider/deep-live-cam \
  --local-dir /raid/projects/deep-wcam/Deep-Live-Cam/models \
  --local-dir-use-symlinks False
```

> Downloads: inswapper_128_fp16.onnx (278MB), GFPGANv1.4.onnx (340MB),
> GPEN-BFR-512.onnx (284MB), hyperswap_1a_256.onnx (403MB),
> xseg.onnx (70MB), Real-ESRGAN-x4plus.onnx (67MB), + others

### Step 4: Create symlink for model filename bug #1628 `[ccs mmhs]`

```bash
cd /raid/projects/deep-wcam/Deep-Live-Cam/models
ln -sf inswapper_128_fp16.onnx inswapper_128.onnx
```

### Step 5: Symlink models to /raid/models/ for cross-project reuse `[ccs mmhs]`

```bash
ln -sf /raid/projects/deep-wcam/Deep-Live-Cam/models/inswapper_128_fp16.onnx \
       /raid/models/vision/deep-live-cam/
ln -sf /raid/projects/deep-wcam/Deep-Live-Cam/models/GFPGANv1.4.onnx \
       /raid/models/vision/deep-live-cam/
ln -sf /raid/projects/deep-wcam/Deep-Live-Cam/models/Real-ESRGAN-x4plus.onnx \
       /raid/models/vision/deep-live-cam/
```

### Step 6: Pre-download InsightFace buffalo_l `[ccs mm — after Phase 1 venv ready]`

```bash
source /raid/projects/deep-wcam/Deep-Live-Cam/.venv/bin/activate
python -c "
from insightface.app import FaceAnalysis
app = FaceAnalysis(name='buffalo_l')
app.prepare(ctx_id=0, det_size=(640, 640))
print('InsightFace buffalo_l loaded OK')
"
```

> This step depends on Phase 1 venv — schedule after venv is ready

### Step 7: Verify all models `[ccs mm]`

```bash
ls -lh /raid/projects/deep-wcam/Deep-Live-Cam/models/*.onnx
echo "---"
ls -lh /raid/models/vision/deep-live-cam/
```

## CCS Delegation Map

| Step | Profile | Reason |
|------|---------|--------|
| 1-5 | `mmhs` | Simple mkdir, download, symlink — no reasoning |
| 6-7 | `mm` | Python execution + verification, moderate complexity |

## Todo

- [x] Create models directories `[ccs mmhs]`
- [x] Install huggingface-hub `[ccs mmhs]`
- [x] Download ALL models from HF (~5.76GB, 13 ONNX models) `[ccs mmhs]`
- [x] Create inswapper_128.onnx symlink (bug #1628) `[ccs mmhs]`
- [x] Symlink to /raid/models/ for reuse `[ccs mmhs]`
- [x] Pre-download InsightFace buffalo_l `[ccs mm, after Phase 1]`
- [x] Verify all models present `[ccs mm]`

## Success Criteria

1. All ONNX models present in `Deep-Live-Cam/models/`
2. `inswapper_128.onnx` symlink resolves to `inswapper_128_fp16.onnx`
3. `/raid/models/vision/deep-live-cam/` contains symlinks
4. InsightFace buffalo_l downloaded to `~/.insightface/models/`

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| HuggingFace download slow/throttled | Low | User authenticated as phenixstar |
| Model file corruption | Low | Verify file sizes match expected |
| Disk space insufficient | Very Low | /raid/ has ample space |

## Security Considerations

- Models from official HuggingFace repo (hacksider/deep-live-cam)
- ONNX is data-only format — no executable code
- Verify file sizes match expected values

## Completion Notes

All steps completed successfully. 13 ONNX models downloaded (5.2GB total), bug #1628 symlink created (inswapper_128.onnx → inswapper_128_fp16.onnx), /raid/models/ cross-project symlinks established, InsightFace buffalo_l pre-downloaded on CUDA.

## Next Steps

Proceed to Phase 3.

---
phase: 1
title: "Clone & Environment Setup"
status: complete
priority: P1
effort: 45m
executor: "team:env-setup + ccs mmhs/mm"
completed: 2026-03-28
---

# Phase 1: Clone & Environment Setup

- **Parent Plan:** [plan.md](plan.md)
- **Dependencies:** None (first phase)
- **Parallel with:** [Phase 2](phase-02-model-setup.md) (model downloads)
- **Date:** 2026-03-28

## Overview

Install Python 3.11 via deadsnakes, clone Deep-Live-Cam into subfolder, create venv, install all deps with GPU support including tensorflow/content filter. Runs as `/team "env-setup"` with subtasks delegated to CCS minimax.

## Key Insights

- Python 3.11 via deadsnakes PPA (user validated — matches upstream docs)
- Clone into `/raid/projects/deep-wcam/Deep-Live-Cam/` subfolder (keeps plans/ isolated)
- `pygrabber` in requirements.txt is Windows-only — must skip on Linux
- `insightface` 0.7.3 requires Cython compilation
- cuDNN 9.1 via pip `nvidia-cudnn-cu12` — no sudo needed
- Keep tensorflow + opennsfw2 (content filter) — user confirmed

## Requirements

- git, gcc, cmake, build-essential (confirmed installed)
- sudo access for deadsnakes PPA (python3.11 install)
- Network access to github.com and pypi.org

## Implementation Steps

### Step 1: Install Python 3.11 `[ccs mmhs]`

```bash
sudo add-apt-repository -y ppa:deadsnakes/ppa
sudo apt update
sudo apt install -y python3.11 python3.11-venv python3.11-dev
python3.11 --version  # verify
```

### Step 2: Clone repo into subfolder `[ccs mmhs]`

```bash
cd /raid/projects/deep-wcam
git clone https://github.com/hacksider/Deep-Live-Cam.git Deep-Live-Cam
```

### Step 3: Create Python 3.11 venv `[ccs mm]`

```bash
cd /raid/projects/deep-wcam/Deep-Live-Cam
python3.11 -m venv .venv
source .venv/bin/activate
pip install --upgrade pip setuptools wheel
```

### Step 4: Install build deps + GPU runtime `[ccs mm]`

```bash
source /raid/projects/deep-wcam/Deep-Live-Cam/.venv/bin/activate
pip install Cython numpy==1.26.4
pip install nvidia-cudnn-cu12==9.1.0.70 nvidia-cublas-cu12
```

### Step 5: Install project requirements (patched) `[ccs mm]`

```bash
cd /raid/projects/deep-wcam/Deep-Live-Cam
# Strip pygrabber (Windows-only), keep everything else including tensorflow
grep -v pygrabber requirements.txt > requirements-linux.txt
pip install -r requirements-linux.txt
```

> If insightface fails: `pip install insightface==0.7.3 --no-build-isolation`

### Step 6: Install PyTorch with CUDA 12 `[ccs mm]`

```bash
pip install -U torch torchvision torchaudio --index-url https://download.pytorch.org/whl/cu121
```

> cu121 — stable wheels; compatible with CUDA 12.0 driver

### Step 7: Configure cuDNN LD_LIBRARY_PATH `[ccs mm]`

```bash
cd /raid/projects/deep-wcam/Deep-Live-Cam
CUDNN_PATH=$(.venv/bin/python -c "import nvidia.cudnn; print(nvidia.cudnn.__path__[0])")
echo "export LD_LIBRARY_PATH=${CUDNN_PATH}/lib:\$LD_LIBRARY_PATH" >> .venv/bin/activate
```

### Step 8: Verify GPU providers `[Claude — needs reasoning on failure]`

```bash
source .venv/bin/activate
python -c "
import onnxruntime as ort
providers = ort.get_available_providers()
print('Providers:', providers)
assert 'CUDAExecutionProvider' in providers, 'CUDA not available!'
import torch; assert torch.cuda.is_available(), 'PyTorch CUDA failed!'
import insightface; print('insightface OK')
print('ALL CHECKS PASSED')
"
```

## CCS Delegation Map

| Step | Profile | Reason |
|------|---------|--------|
| 1-2 | `mmhs` | Simple apt/git commands, no reasoning needed |
| 3-7 | `mm` | pip installs + config edits, moderate complexity |
| 8 | Claude | Verification needs reasoning to debug failures |

## Todo

- [x] Install Python 3.11 via deadsnakes `[ccs mmhs]`
- [x] Clone repo into Deep-Live-Cam/ subfolder `[ccs mmhs]`
- [x] Create Python 3.11 venv `[ccs mm]`
- [x] Install build deps + GPU runtime `[ccs mm]`
- [x] Patch requirements and install `[ccs mm]`
- [x] Install PyTorch CUDA `[ccs mm]`
- [x] Configure cuDNN LD_LIBRARY_PATH `[ccs mm]`
- [x] Verify CUDAExecutionProvider `[Claude]`

## Success Criteria

1. `python3.11 --version` returns 3.11.x
2. `onnxruntime.get_available_providers()` includes `CUDAExecutionProvider`
3. `import insightface` succeeds
4. `torch.cuda.is_available()` returns True
5. All imports from requirements.txt work (except pygrabber)

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| insightface build failure | Medium | Pre-install Cython; use --no-build-isolation |
| onnxruntime/cuDNN mismatch | Medium | Pin nvidia-cudnn-cu12==9.1.0.70; set LD_LIBRARY_PATH |
| deadsnakes PPA unavailable | Low | Fallback to python3.10 (already installed) |
| tensorflow install timeout | Low | Retry with --timeout 300 |

## Security Considerations

- Cloning from public GitHub repo — verify commit hash after clone
- venv isolates all packages from system Python
- deadsnakes PPA is well-known trusted source

## Completion Notes

All steps completed successfully. Python 3.11.15 installed, repo cloned to Deep-Live-Cam/ subfolder, venv created with all deps, tensorflow/opennsfw2 content filter retained, CUDA verified with TensorRT + CUDA + CPU providers, python3.11-tk added for GUI support.

## Next Steps

Proceed to Phase 3 after Phase 2 completes.

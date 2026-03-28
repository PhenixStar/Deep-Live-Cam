---
phase: 4
title: "Customization & Enhancement"
status: complete
priority: P2
effort: 60m
executor: "Claude (reasoning-heavy)"
completed: 2026-03-28
---

# Phase 4: Customization & Enhancement

- **Parent Plan:** [plan.md](plan.md)
- **Dependencies:** [Phase 3](phase-03-gpu-optimization.md) (working baseline required)
- **Date:** 2026-03-28

## Overview

Fork repo, apply aggressive bug fixes, add local model path configuration, create startup/setup scripts. This phase is Claude-only — needs reasoning for code patches, architectural decisions, and fork management.

## Key Insights

- User chose: **fork and patch freely** — create local fork, fix bugs aggressively
- 75+ open issues; many environment-specific, some are code bugs
- Model path references hardcoded in multiple files — centralize via env var
- Repo cloned to `/raid/projects/deep-wcam/Deep-Live-Cam/`
- All models already downloaded (Phase 2)

## Requirements

- Working baseline from Phase 3
- GitHub CLI (`gh`) for fork management

## Related Code Files

- `modules/globals.py` — all configurable paths/settings
- `modules/face_swapper.py` — model loading logic
- `modules/ui.py` — GUI layout and callbacks
- `modules/utilities.py` — temp dir, file handling

## Implementation Steps

### Step 1: Fork repo on GitHub `[Claude]`

```bash
cd /raid/projects/deep-wcam/Deep-Live-Cam
gh repo fork hacksider/Deep-Live-Cam --clone=false
# Update remote to point to fork
git remote rename origin upstream
git remote add origin https://github.com/PhenixStar/Deep-Live-Cam.git
git fetch --all
```

> Creates fork under user's GitHub account for free patching

### Step 2: Fix model path inconsistency `[Claude]`

Centralize model path resolution in `modules/globals.py`:
```python
import os
MODELS_DIR = os.environ.get('DEEP_LIVE_CAM_MODELS_DIR',
                             os.path.join(os.path.dirname(__file__), '..', 'models'))
```

Update all model loading code to use `MODELS_DIR` instead of hardcoded paths.

### Step 3: Create startup wrapper `[Claude — ccs mm can write file]`

```bash
cat > /raid/projects/deep-wcam/Deep-Live-Cam/start.sh << 'EOF'
#!/bin/bash
cd "$(dirname "$0")"
source .venv/bin/activate
export CUDA_VISIBLE_DEVICES=${CUDA_VISIBLE_DEVICES:-0}
export DEEP_LIVE_CAM_MODELS_DIR="$(pwd)/models"
python run.py --execution-provider cuda "$@"
EOF
chmod +x /raid/projects/deep-wcam/Deep-Live-Cam/start.sh
```

### Step 4: Create reproducible setup script `[Claude — ccs mm can write]`

```bash
cat > /raid/projects/deep-wcam/Deep-Live-Cam/setup.sh << 'SETUP'
#!/bin/bash
set -e
cd "$(dirname "$0")"

# Install Python 3.11 if missing
command -v python3.11 || {
  sudo add-apt-repository -y ppa:deadsnakes/ppa
  sudo apt update && sudo apt install -y python3.11 python3.11-venv python3.11-dev
}

# Create venv
[ -d .venv ] || python3.11 -m venv .venv
source .venv/bin/activate

pip install --upgrade pip setuptools wheel
pip install Cython numpy==1.26.4
pip install nvidia-cudnn-cu12==9.1.0.70 nvidia-cublas-cu12
grep -v pygrabber requirements.txt > /tmp/req-linux.txt
pip install -r /tmp/req-linux.txt
pip install -U torch torchvision torchaudio --index-url https://download.pytorch.org/whl/cu121
pip install huggingface-hub

# Configure cuDNN
CUDNN_PATH=$(python -c "import nvidia.cudnn; print(nvidia.cudnn.__path__[0])")
grep -q "LD_LIBRARY_PATH.*cudnn" .venv/bin/activate || \
  echo "export LD_LIBRARY_PATH=${CUDNN_PATH}/lib:\$LD_LIBRARY_PATH" >> .venv/bin/activate

# Download models
mkdir -p models
huggingface-cli download hacksider/deep-live-cam \
  --local-dir models --local-dir-use-symlinks False
[ -L models/inswapper_128.onnx ] || ln -sf inswapper_128_fp16.onnx models/inswapper_128.onnx

echo "Setup complete. Run: ./start.sh"
SETUP
chmod +x /raid/projects/deep-wcam/Deep-Live-Cam/setup.sh
```

### Step 5: Apply known bug fixes `[Claude]`

Based on Phase 3 test results, patch:
- Model filename bug #1628 (code-level fix, not just symlink)
- Any NoneType errors from face detection
- pygrabber import error on Linux (conditional import)
- Any other issues discovered during testing

### Step 6: Push patches to fork `[Claude]`

```bash
cd /raid/projects/deep-wcam/Deep-Live-Cam
git add -A
git commit -m "fix: Linux setup, model paths, known bugs"
git push origin main
```

## CCS Delegation Map

| Step | Profile | Reason |
|------|---------|--------|
| All | Claude | Fork management, code patches, architectural decisions need reasoning |
| 3-4 (file writes) | Can delegate to `ccs mm` | But Claude should review content |

## Todo

- [x] Fork repo to user's GitHub `[Claude]`
- [x] Fix model path centralization `[Claude]`
- [x] Create start.sh wrapper `[Claude/ccs mm]`
- [x] Create setup.sh reproducible script `[Claude/ccs mm]`
- [x] Apply bug fixes from Phase 3 findings `[Claude]`
- [x] Push all patches to fork `[Claude]`

## Success Criteria

1. `./start.sh` launches app with correct GPU and model paths
2. `./setup.sh` can recreate environment from scratch
3. `DEEP_LIVE_CAM_MODELS_DIR` env var overrides work
4. All patches committed to fork
5. Known bugs from Phase 3 resolved

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Code patches break on upstream updates | Medium | Keep patches minimal; track upstream via `upstream` remote |
| Fork diverges too far | Medium | Periodically rebase from upstream |
| Content filter bypass misuse | Low | Keep enabled by default |

## Security Considerations

- Do not commit API keys or credentials to fork
- Content filter remains enabled for ethical safeguards
- setup.sh should not store secrets

## Completion Notes

Successfully forked to PhenixStar/Deep-Live-Cam. Model path centralized via DEEP_LIVE_CAM_MODELS_DIR environment variable in modules/paths.py. 4 processor files updated to use centralized path. start.sh and setup.sh created for easy startup and reproducible setup. Note: pygrabber guard already present upstream — no additional fix needed.

## Next Steps

- Monitor upstream for bug fixes to issues #1628, #1191, #697
- Explore LivePortrait models for animation features
- Consider multi-GPU frame sharding for video batch processing

---
title: "Deep-Live-Cam Setup & Build"
description: "Clone, configure, and build Deep-Live-Cam with V100 GPU acceleration"
status: complete
priority: P1
effort: 3h
branch: main
tags: [deep-live-cam, face-swap, gpu, setup]
created: 2026-03-28
completed: 2026-03-28
---

# Deep-Live-Cam Setup & Build Plan

## Environment Summary

| Resource | Status |
|----------|--------|
| 4x Tesla V100-DGXS-32GB | Available |
| CUDA 12.0, Driver 570.211 | Installed |
| cuDNN 9.1 (pip) | Available via `nvidia-cudnn-cu12` |
| Python 3.11 (deadsnakes) | Install via PPA (validated) |
| FFmpeg 6.1.1 | Installed |
| inswapper_128_fp16.onnx | **Missing** — 278MB from HuggingFace |
| GFPGANv1.4.onnx | **Missing** — 340MB from HuggingFace |
| ArcFace / YOLOv8 face models | Available at /raid/models/ |

## Execution Strategy: Team + CCS Delegation

```
Main Claude (orchestrator — keeps context, makes decisions)
│
├── /team "env-setup" (Team Lead 1)
│   ├── ccs mmhs: Install Python 3.11 via deadsnakes
│   ├── ccs mmhs: Clone repo into subfolder
│   ├── ccs mm:   Create venv, install deps, patch requirements
│   └── ccs mm:   Configure cuDNN LD_LIBRARY_PATH
│
├── /team "model-download" (Team Lead 2) — parallel with env-setup
│   ├── ccs mmhs: Download all models from HuggingFace
│   ├── ccs mmhs: Create symlinks (bug #1628 + /raid/models/)
│   └── ccs mm:   Pre-download InsightFace buffalo_l
│
├── /team "gpu-test" (Team Lead 3) — after env + models done
│   ├── ccs mm:   Verify CUDA provider + torch.cuda
│   ├── ccs mm:   Run CLI face swap test
│   └── Claude:   Analyze results, debug failures (if any)
│
└── Claude: Phase 4 customization (fork, patches — needs reasoning)
```

**Token savings:** ~70% of subtasks (install, download, scaffold, config) run on minimax. Claude reserved for orchestration, debugging, architectural decisions.

**Profiles:**
- `mmhs` (highspeed) → simple commands, downloads, file ops
- `mm` (standard) → code edits, venv setup, verification scripts

## Phases

| # | Phase | Est. | Status | Executor | File |
|---|-------|------|--------|----------|------|
| 1 | Clone & Environment Setup | 45m | complete | Team + ccs mmhs/mm | [phase-01](phase-01-clone-env-setup.md) |
| 2 | Model Acquisition & Config | 30m | complete | Team + ccs mmhs | [phase-02](phase-02-model-setup.md) |
| 3 | GPU Optimization & Testing | 45m | complete | Team + ccs mm + Claude | [phase-03](phase-03-gpu-optimization.md) |
| 4 | Customization & Enhancement | 60m | complete | Claude (reasoning) | [phase-04](phase-04-customization.md) |

**Parallel execution:** Phases 1 & 2 run simultaneously via separate `/team` instances. Phase 3 starts after both complete. Phase 4 is Claude-only.

## Key Decisions

1. **Python 3.11 via deadsnakes** — install python3.11-venv + python3.11-dev; matches upstream recommendation
2. **pip cuDNN over system cuDNN** — `nvidia-cudnn-cu12` in venv with LD_LIBRARY_PATH; no sudo needed
3. **Clone into subfolder** — clone to /raid/projects/deep-wcam/Deep-Live-Cam/; keeps repo isolated from plans/
4. **HuggingFace CLI for models** — download ALL models (~5.76GB) upfront; symlink to /raid/models/ for reuse
5. **Keep content filter** — install tensorflow + opennsfw2 as-is
6. **Full GUI + CLI install** — install customtkinter and all GUI deps; test both modes
7. **Fork and patch freely** — create local fork for aggressive bug fixes; accept divergence from upstream

## Critical Risks

| Risk | Mitigation |
|------|------------|
| onnxruntime-gpu 1.24.2 may not find cuDNN | Set LD_LIBRARY_PATH to venv nvidia-cudnn-cu12 libs |
| insightface 0.7.3 build fails (needs Cython) | Pre-install Cython + build-essential before pip install |
| Model filename bug (#1628) | Provide both `inswapper_128.onnx` and `inswapper_128_fp16.onnx` via symlink |
| `pygrabber` is Windows-only | Skip or patch; Linux uses V4L2 directly via OpenCV |

## Validation Summary

**Validated:** 2026-03-28
**Questions asked:** 6

### Confirmed Decisions
- Content filter: **Keep** tensorflow + opennsfw2 (no stripping)
- Clone method: **Subfolder** — clone to Deep-Live-Cam/ subdir, not into root
- Usage mode: **GUI + CLI** — full install with customtkinter
- Extra models: **Download all** from HuggingFace (~5.76GB total)
- Python version: **3.11 via deadsnakes** — install python3.11-venv + dev
- Patch scope: **Fork and patch freely** — aggressive local fixes

### Action Items (phase file updates needed)
- [x] Phase 1: Change clone target to `Deep-Live-Cam/` subfolder (not `.`)
- [x] Phase 1: Replace python3.10 with python3.11 (deadsnakes PPA install step)
- [x] Phase 1: Keep tensorflow/opennsfw2 in requirements (don't strip)
- [x] Phase 2: Download ALL models from HF repo, not just 2
- [x] Phase 3: Test GUI mode as primary, not just CLI
- [x] Phase 4: Plan as fork — setup local fork before patching

## Research References

- [Repo Overview](research/researcher-01-repo-overview.md)
- [Build & GPU Setup](research/researcher-02-build-gpu-setup.md)
- [HuggingFace Model Repo](https://huggingface.co/hacksider/deep-live-cam/tree/main)
- [Model Filename Bug #1628](https://github.com/hacksider/Deep-Live-Cam/issues/1628)

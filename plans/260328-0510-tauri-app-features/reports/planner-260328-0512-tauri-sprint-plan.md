# Report: Tauri App Feature Sprint Plan

**Date:** 2026-03-28 | **Author:** planner | **Status:** Complete

## Summary

Created a 3-phase implementation plan for 4 Tauri app features spanning frontend (React+TS), backend (Python FastAPI), bundling (python-build-standalone), and DevOps (GitHub Actions CI/CD). Total estimated effort: 12h across 3 teams.

## Deliverables

| File | Description |
|------|-------------|
| `plan.md` | Master plan with architecture, team structure, cross-team deps, risk summary |
| `phase-01-camera-enhancer-ui.md` | Camera dropdown + face enhancer toggles (12 steps, API contract defined first) |
| `phase-02-python-bundle.md` | python-build-standalone + venv sidecar bundling (8 steps, 3 platform scripts) |
| `phase-03-installers-cicd.md` | GitHub Actions release + CI workflows, auto-update plugin (8 steps) |

## Key Findings from Codebase Analysis

1. **Camera enumeration already exists** -- `modules/ui.py:get_available_cameras()` (lines 1013-1072) handles Windows/Linux/macOS. Needs extraction to `camera_utils.py` for server.py reuse.
2. **Enhancer toggle infrastructure already exists** -- `globals.fp_ui` dict and `set_frame_processors_modules_from_ui()` in `core.py` hot-swap enhancer modules. Server just needs to expose it via HTTP.
3. **Server.py WS loop needs two changes** -- use `_active_camera_index` instead of hardcoded `0`; call `get_frame_processors_modules()` inside the loop instead of once before it.
4. **python-build-standalone + venv is 200-250 MB** (no models), well under the 2 GB GitHub Release limit. Models download on first run.
5. **Windows sidecar needs a tiny Rust launcher** (~50 lines) because Tauri `externalBin` expects a real executable, not a batch file.

## CCS Delegation Summary

- **28 of 32 steps** delegable to mmu agents (config changes, standard endpoints, mechanical translations, boilerplate)
- **4 steps** require Team Leader execution: WS loop integration (thread safety), signing keypair generation, release workflow authoring, sidecar path verification

## Unresolved Questions

1. Model distribution URLs -- which Hugging Face repos to use for ONNX model downloads?
2. Windows launcher approach -- compiled Rust binary vs `.cmd` batch workaround?
3. macOS targets -- ARM64 only or also x86_64 Intel?
4. GitHub repo URL for updater endpoint (determines `latest.json` path)
5. `onnxruntime-silicon` vs `onnxruntime` (CPU) as macOS ARM64 default

---
title: "Tauri App Feature Sprint"
description: "Camera selection, face enhancers, portable Python bundle, cross-platform installers"
status: in-progress
priority: P1
effort: 12h
branch: main
tags: [tauri, react, fastapi, python-bundle, installers, ci-cd]
created: 2026-03-28
---

# Tauri App Feature Sprint

## Overview

Four features to evolve the Deep Live Cam desktop app from dev prototype to shippable product:

1. **Camera Selection Dropdown** -- let users pick which webcam to use
2. **Face Enhancer Toggles** -- GFPGAN, GPEN-256, GPEN-512 on/off switches
3. **Portable Python Bundle** -- ship python-build-standalone so users need zero Python install
4. **Cross-Platform Installers** -- MSI/DMG/AppImage via GitHub Actions CI/CD

## Current State

| Layer | File | Key Details |
|-------|------|-------------|
| Frontend | `deep-live-cam-app/src/App.tsx` | React 19 + Vite 8, canvas-based WS video, source upload, connect/disconnect |
| Tauri | `src-tauri/src/main.rs` | Spawns `binaries/deep-live-cam-server` sidecar, exposes `get_backend_url` command |
| Config | `src-tauri/tauri.conf.json` | externalBin: `binaries/deep-live-cam-server`, shell plugin with sidecar scope |
| Backend | `Deep-Live-Cam/server.py` | FastAPI on :8008, `/health`, `POST /source`, `WS /ws/video` (hardcoded camera 0) |
| Globals | `modules/globals.py` | `fp_ui` dict already has `face_enhancer`, `face_enhancer_gpen256`, `face_enhancer_gpen512` toggles |
| Processors | `modules/processors/frame/core.py` | `set_frame_processors_modules_from_ui()` already reads `fp_ui` to hot-add/remove enhancers |
| Camera util | `modules/ui.py:get_available_cameras()` | Platform-aware camera enumeration (Windows=DirectShow, Linux=probe 0-9, macOS=static [0,1]) |
| Deps | `requirements.txt` | `cv2_enumerate_cameras`, `onnxruntime-gpu`, `insightface`, `opencv-python` |

## Architecture Diagram

```
+------------------+     HTTP/WS      +-------------------+
|  Tauri Webview   | <--------------> |  FastAPI Server    |
|  (React + TS)    |   localhost:8008  |  (Python sidecar)  |
+------------------+                  +-------------------+
        |                                     |
  Tauri Shell Plugin               cv2.VideoCapture(idx)
  spawns sidecar                   ONNX face_swapper
                                   ONNX face_enhancers
```

## Phases

| Phase | Features | Effort | Status | Phase Doc |
|-------|----------|--------|--------|-----------|
| 1 | Camera selection + Face enhancer toggles (frontend + backend) | 4h | **COMPLETE** | [phase-01](phase-01-camera-enhancer-ui.md) |
| 2 | python-build-standalone bundling | 4h | pending | [phase-02](phase-02-python-bundle.md) |
| 3 | CI/CD, installers, auto-update | 4h | pending | [phase-03](phase-03-installers-cicd.md) |

Phases are sequential: Phase 2 depends on Phase 1 (needs working backend endpoints). Phase 3 depends on Phase 2 (needs bundled sidecar to package).

## Team Structure

```
Team 1 (Frontend)          Team 2 (Backend)            Team 3 (DevOps)
  Leader + 5 mmu             Leader + 5 mmu              Leader + 5 mmu
  +-Camera dropdown UI       +-GET /cameras endpoint      +-GitHub Actions workflow
  +-Enhancer toggle UI       +-POST /camera/{idx}          +-tauri.conf.json bundle cfg
  +-API integration           +-POST /settings               +-Code signing setup
  +-Error states              +-WS enhancer pipeline         +-Auto-update manifest
  +-CSS/layout                +-python-build-standalone      +-Release automation
```

## Cross-Team Dependencies

- Team 1 blocked on Team 2 delivering API contract (resolve: define OpenAPI spec first)
- Team 3 blocked on Team 2 delivering bundle script (resolve: can scaffold CI in parallel using stub sidecar)

## Risk Summary

| Risk | Impact | Mitigation |
|------|--------|------------|
| macOS camera probe SIGSEGV | Crash on macOS | Use static camera list on Darwin (existing pattern) |
| ONNX model files 300-500MB each | Installer too large | Download models on first run, not bundled |
| python-build-standalone + venv > 300MB | Large download | Compress with zstd; exclude unused stdlib modules |
| GitHub Actions runners lack GPU | Cannot test GPU path | CPU-only CI tests; manual GPU QA |
| Code signing certs cost money | Unsigned warnings on Win/Mac | Optional for v1; document manual signing process |

## Validated Decisions (2026-03-28)

| Question | Decision |
|----------|----------|
| Camera sources | PC webcam primary; future: Android remote, video file, RTSP/IP |
| Model distribution | Auto-download on first run (A); keep skeleton for bundled option (C) for future APK |
| Windows sidecar | python-build-standalone + venv (A) — portable, updatable |
| GitHub repo | PhenixStar/Deep-Live-Cam (A); restructure: core/ + app/ folders, fix all refs |
| Platform priority | Windows → Linux → macOS |
| Repo structure | Monorepo: `core/` (Python inference) + `app/` (Tauri desktop), same repo |

## Folder Restructure (Pre-Implementation)

```
PhenixStar/Deep-Live-Cam/
├── core/                 # Python inference engine (current modules/, server.py)
│   ├── modules/
│   ├── models/
│   ├── server.py
│   ├── run.py
│   ├── setup.sh
│   └── requirements.txt
├── app/                  # Tauri desktop app (current deep-live-cam-app/)
│   ├── src/              # React frontend
│   ├── src-tauri/        # Rust backend
│   └── package.json
├── scripts/              # Build, bundle, CI helpers
└── README.md
```

**Note:** Folder restructure must happen BEFORE Phase 1 implementation to avoid path breakage mid-sprint.

## Success Criteria (Definition of Done)

1. User can select camera from dropdown; video stream switches to chosen camera
2. User can toggle GFPGAN/GPEN-256/GPEN-512; enhancers apply in real-time on WS frames
3. App bundles portable Python + venv; no system Python required to run
4. `pnpm tauri build` produces MSI (Windows) and AppImage (Linux)
5. GitHub Actions workflow builds and publishes release artifacts on tag push
6. Model auto-download on first run with progress indicator

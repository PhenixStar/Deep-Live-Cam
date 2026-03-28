# Research Report: Deep-Live-Cam as Cross-Platform Compiled App & Plugin

**Date:** 2026-03-28
**Sources consulted:** 12 (Gemini, WebSearch, GitHub repos, crate docs)
**Key terms:** ort crate, PyInstaller, Nuitka, Tauri, OBS plugin, virtual camera, v4l2loopback, ONNX Runtime Rust

---

## Executive Summary

Three viable paths exist for shipping Deep-Live-Cam as a compiled cross-platform app. **Recommendation: Hybrid Rust+Python via Tauri** for the near-term, with a long-term migration to pure Rust using the `ort` crate. For plugins, **virtual camera output** is the highest-impact, lowest-effort approach — works with OBS, Zoom, Teams, Discord simultaneously.

---

## Part 1: Cross-Platform Compiled App

### Approach Comparison

| Approach | Rewrite Effort | Binary Size | Startup | GPU Support | Distribution |
|----------|:---:|:---:|:---:|:---:|:---:|
| **PyInstaller** (Python bundle) | None | 800MB-2GB | 3-8s | Complex (DLL hell) | Hard |
| **Nuitka** (Python→C compile) | None | 400MB-1GB | 1-3s | Same DLL issues | Medium |
| **Tauri + Python backend** | Low (UI only) | 50MB + Python | 1-2s | Delegated to Python | Medium |
| **Rust + PyO3** (Rust wrapper) | Medium | 100-200MB | 0.5s | Delegated to Python | Medium |
| **Pure Rust + `ort`** | High (full rewrite) | 30-80MB | 0.3s | Native (best) | Easy |

### 1. PyInstaller / Nuitka (Python Bundling)

**Verdict: Fragile for GPU apps, acceptable for quick distribution.**

Pros:
- Zero code changes needed
- Mature tooling, large community

Cons:
- ONNX Runtime GPU bundling is notoriously broken ([pyinstaller#8083](https://github.com/pyinstaller/pyinstaller/issues/8083), [nuitka#2029](https://github.com/Nuitka/Nuitka/issues/2029))
- CUDA DLLs in `onnxruntime/capi` get missed — "Init provider bridge failed" errors
- Cannot cross-compile (must build on each target platform)
- Binary size 800MB+ with tensorflow/CUDA included
- Must ship separate builds for CUDA (NVIDIA), DirectML (AMD/Intel), CoreML (Mac)

Workaround for GPU:
```python
# Hidden imports for PyInstaller
# hook-onnxruntime.py
from PyInstaller.utils.hooks import collect_dynamic_libs
datas, binaries, hiddenimports = collect_dynamic_libs('onnxruntime')
```

### 2. Tauri + Python Backend (Recommended Near-Term)

**Verdict: Best balance of effort vs. polish for a consumer app.**

Architecture:
```
┌─────────────────────────────────┐
│  Tauri App (Rust core)          │
│  ├── Web UI (React/Svelte)      │
│  ├── System tray, auto-update   │
│  └── Sidecar: Python process    │
│       ├── ONNX inference        │
│       ├── OpenCV video capture  │
│       └── WebSocket → UI        │
└─────────────────────────────────┘
```

Pros:
- Modern UI with web tech (React/Vue/Svelte)
- Rust core handles system integration (tray, updates, permissions)
- Python sidecar runs existing Deep-Live-Cam code unchanged
- Auto-update via Tauri's built-in updater
- Binary: ~50MB Rust + bundled Python (~200MB total)
- Cross-platform: Windows/Mac/Linux from same codebase

Cons:
- Must bundle Python runtime (or require user install)
- Two-process architecture adds IPC complexity
- GPU setup still on the user

Key tech:
- `tauri::api::process::Command::new_sidecar()` for Python process
- WebSocket or stdout pipe for frame data
- Bundle Python via `python-build-standalone` (single-file Python)

### 3. Pure Rust + `ort` Crate (Long-Term Best)

**Verdict: Best end result, but 2-4 month rewrite.**

The `ort` crate v2.0+ (2025) is production-ready:
- Links ONNX Runtime v1.17-v1.24
- GPU providers: CUDA (Linux/Win), DirectML (Win), CoreML (Mac), WebGPU (all)
- `load-dynamic` feature for graceful CPU fallback
- 1.3x throughput, 4x startup, 10x less memory vs Python

Full rewrite scope:
```
Python → Rust Module Mapping
─────────────────────────────
face_analyser.py  → rust-insightface (SCRFD detection + ArcFace recognition)
face_swapper.py   → ort session + ndarray preprocessing
face_enhancer.py  → ort session for GFPGAN/GPEN
core.py           → tokio async pipeline
ui.py             → egui/iced/tauri frontend
video_capture     → nokhwa crate (cross-platform camera)
image processing  → kornia-rs (3-5x faster than image crate)
```

Example preprocessing in Rust:
```rust
use ort::{Session, SessionBuilder, Value};
use ndarray::Array4;

fn run_inswapper(session: &Session, face: Array4<f32>, embedding: Array1<f32>) -> Array4<f32> {
    let inputs = vec![
        Value::from_array(face).unwrap(),
        Value::from_array(embedding).unwrap(),
    ];
    let outputs = session.run(inputs).unwrap();
    outputs[0].extract_tensor::<f32>().unwrap().to_owned()
}
```

Packaging with `cargo-dist`:
1. Enable `load-dynamic` in Cargo.toml
2. Bundle `onnxruntime.dll/so/dylib` in `libs/`
3. Use `ort::util::preload_dylib()` at startup
4. Ship platform-specific installers (MSI/DMG/AppImage)

### 4. Rust + PyO3 Hybrid

**Verdict: Worst of both worlds for this use case. Skip.**

Embeds Python in Rust via FFI. Still needs Python runtime, still has GIL limitations. Only useful if you want a Rust CLI wrapper around existing Python code — but Tauri sidecar is simpler.

---

## Part 2: Plugin Architectures

### Plugin Feasibility Matrix

| Plugin Target | Effort | GPU Access | Real-Time | Distribution | Reach |
|--------------|:---:|:---:|:---:|:---:|:---:|
| **Virtual Camera** (v4l2/DirectShow) | Low | Yes (own process) | Yes | Self-hosted | Massive |
| **OBS Source Plugin** | Medium | Yes (shared GPU) | Yes | OBS store | Large |
| **Browser Extension** (WebRTC) | High | WebGPU only | Marginal | Chrome Store | Huge |
| **Zoom/Teams SDK** | High | Limited | Yes | App store | Large |
| **OpenFX** (DaVinci/Nuke) | Medium | Yes | Offline | Self-hosted | Niche |
| **After Effects Plugin** | High | Limited | Offline | Adobe Exchange | Medium |
| **Unity/Unreal** | Medium | Yes | Yes | Asset Store | Niche |
| **NDI Source** | Low | Yes (own process) | Yes | Self-hosted | Medium |

### 1. Virtual Camera Output (Highest Impact)

**Verdict: Build this first. Works everywhere with zero integration effort.**

Architecture:
```
Deep-Live-Cam Process
├── Camera capture (real webcam)
├── Face swap (ONNX/CUDA)
├── Write to virtual camera device
│   ├── Linux: v4l2loopback (/dev/videoN)
│   ├── Windows: DirectShow virtual camera (OBS-VirtualCam style)
│   └── macOS: CoreMediaIO Camera Extension (macOS 13+)
└── Any app sees it as "Deep-Live-Cam Camera"
    ├── Zoom, Teams, Meet, Discord
    ├── OBS Studio
    └── Any video call app
```

Implementation:
- **Linux:** `v4l2loopback` kernel module + write frames via `ioctl(fd, VIDIOC_QBUF, ...)`
- **Windows:** Register a DirectShow source filter (or use `pyvirtualcam` Python package)
- **macOS:** CoreMediaIO Camera Extension (Swift, requires codesigning)
- **Python shortcut:** `pyvirtualcam` package handles all 3 platforms

```python
# Minimal virtual camera integration
import pyvirtualcam
with pyvirtualcam.Camera(width=1280, height=720, fps=30) as cam:
    while True:
        frame = capture_and_swap()  # existing pipeline
        cam.send(frame)
```

Effort: ~50 lines of code to add to existing Deep-Live-Cam.

### 2. OBS Studio Source Plugin

**Verdict: High value for streamers, medium effort.**

Two approaches:
- **Python script plugin** (easiest): OBS has built-in Python scripting. Register a source that captures webcam → runs face swap → outputs frames. Limited by GIL.
- **C/C++ plugin** (better): Use OBS Plugin API (`obs_register_source()`). Link against ONNX Runtime C API directly. Full GPU access, no GIL.
- **Lua script** (limited): Too slow for ML inference.

Existing examples:
- [obs-v4l2sink](https://github.com/CatxFish/obs-v4l2sink) — output plugin pattern
- [Face Changer](https://obsproject.com/forum/resources/face-changer-webcam-effects.1912/) — commercial OBS face effect plugin

### 3. NDI Source

**Verdict: Great for studio/broadcast use. Low effort.**

Use NDI SDK to broadcast the face-swapped feed as an NDI source on the local network. Any NDI receiver (OBS, vMix, Wirecast, TriCaster) can receive it.

```python
import NDIlib as ndi
# Send face-swapped frames as NDI source
sender = ndi.send_create(ndi.SendCreate(ndi_name="Deep-Live-Cam"))
ndi.send_send_video_v2(sender, frame)
```

### 4. Browser Extension (WebRTC Intercept)

**Verdict: Technically possible but impractical for real-time ML.**

Would intercept `getUserMedia()` and replace video track with processed frames. WebGPU ONNX inference in browser is real but latency is 50-100ms per frame — too slow for live video calls. Only viable for non-real-time filters.

### 5. OpenFX Plugin (DaVinci Resolve / Nuke)

**Verdict: Good for post-production workflows.**

OpenFX is the standard plugin API for video compositing apps. Write a C++ plugin that loads ONNX models and processes frames. DaVinci Resolve (free tier) supports OpenFX.

### 6. Zoom/Teams/Discord SDKs

**Verdict: Not feasible — SDKs don't support custom video processing.**

These apps don't expose a plugin API for video frame manipulation. The virtual camera approach bypasses this limitation entirely — they see it as a regular webcam.

---

## Part 3: Recommended Strategy

### Phase 1: Virtual Camera (1-2 weeks)
- Add `pyvirtualcam` to existing Python codebase
- Produces a "Deep-Live-Cam Camera" device
- Instantly works with Zoom, Teams, OBS, Discord, Meet
- Ship as Python app (existing setup.sh)

### Phase 2: Tauri Desktop App (1-2 months)
- Wrap existing Python in Tauri sidecar
- Modern UI (React + Tailwind)
- Auto-update, system tray, one-click install
- Bundle `python-build-standalone` for zero-dep install
- Ship MSI (Windows), DMG (Mac), AppImage (Linux)

### Phase 3: Pure Rust Core (3-6 months)
- Rewrite inference pipeline in Rust + `ort` v2.0
- Replace OpenCV with `kornia-rs` + `nokhwa`
- Keep Tauri frontend
- Result: 30-80MB binary, 0.3s startup, native GPU
- Publish on GitHub Releases + Homebrew + winget

### Phase 4: OBS Plugin (optional, 1 month)
- C/Rust OBS source plugin using ONNX Runtime C API
- Direct GPU access, zero-copy frame pipeline
- Distribute via OBS Plugin Store

---

## Unresolved Questions

1. **InsightFace license:** InsightFace models are non-commercial. Would a compiled distribution violate this? Need legal review.
2. **ONNX model compatibility:** `ort` v2.0 Rust — have inswapper_128 and GFPGAN been tested? No public examples found.
3. **macOS virtual camera:** CoreMediaIO Camera Extension requires Apple Developer codesigning ($99/yr). `pyvirtualcam` uses OBS's virtual camera on macOS as workaround.
4. **WebGPU maturity:** ONNX Runtime WebGPU provider is "new in 2025" — production readiness unclear for real-time inference.

## Sources

- [ort crate (Rust ONNX Runtime)](https://crates.io/crates/ort)
- [PyInstaller ONNX issues](https://github.com/pyinstaller/pyinstaller/issues/8083)
- [Nuitka ONNX GPU issues](https://github.com/Nuitka/Nuitka/issues/2029)
- [Tauri Sidecar API](https://v2.tauri.app/develop/sidecar/)
- [pyvirtualcam](https://github.com/letmaik/pyvirtualcam)
- [obs-v4l2sink](https://github.com/CatxFish/obs-v4l2sink)
- [kornia-rs](https://github.com/kornia/kornia-rs)
- [nokhwa camera crate](https://crates.io/crates/nokhwa)
- [Real-Time-FaceSwap-with-ONNX](https://github.com/dykuang/Real-Time-FaceSwap-with-ONNX)
- [Amigo AI (commercial face swap)](https://www.amigoai.io/guide)
- [MacFaceSwap virtual camera](https://macfaceswap.com/blog/using-macfaceswap-with-virtual-camera/)
- [ONNX Runtime Build EPs](https://onnxruntime.ai/docs/build/eps.html)

# Python-Build-Standalone + Tauri v2 Bundle Research

**Date:** 2026-03-28 | **Scope:** Portable Python runtime for desktop ML app

## 1. Python-Build-Standalone Releases

**Latest Status (2026):**
- Latest release: 20260127 (Jan 27, 2026)
- Includes OpenSSL 3.5.5, VS 2026 support
- Python 3.11.x versions available (3.11.1–3.11.14 confirmed)
- **Source:** [astral-sh/python-build-standalone](https://github.com/astral-sh/python-build-standalone/releases)

**Download Profile:**
- Platform-specific builds: Windows, Linux (glibc/musl), macOS (Intel + Apple Silicon)
- Typical runtime size: ~100–150 MB per platform (Python core + stdlib)
- Pre-built with OpenSSL, SQLite, zlib included
- **No installation required** — extract and run

## 2. Tauri v2 Sidecar Integration

**Configuration:**
- Add binaries to `tauri.conf.json` under `tauri.bundle.externalBin`
- Requires platform-specific naming: `binary-x86_64-unknown-linux-gnu`, `binary-aarch64-apple-darwin`, etc.
- Call via `shell().sidecar()` from Rust side (not full path, just filename)

**Directory Layout:**
```
src-tauri/
├── binaries/
│   ├── python-x86_64-unknown-linux-gnu/
│   │   └── bin/python3
│   ├── python-aarch64-apple-darwin/
│   │   └── bin/python3
│   └── python-x86_64-pc-windows-msvc/
│       └── python.exe
├── tauri.conf.json
└── src/main.rs
```

**Execution Model:**
- Sidecar runs as independent subprocess
- Communication via IPC or HTTP (common: FastAPI/aiohttp server listening on localhost)
- Example: [dieharders/example-tauri-v2-python-server-sidecar](https://github.com/dieharders/example-tauri-v2-python-server-sidecar)

## 3. Bundling ML Dependencies

**Approach:** Use venv inside standalone Python distribution
- Copy `python-build-standalone` directory → bundle as sidecar
- Create venv: `./python/bin/python -m venv ./venv`
- Install packages: `./venv/bin/pip install onnxruntime insightface opencv-python`
- Include venv in sidecar bundle

**Package Sizes (approx):**
- `onnxruntime`: ~50–80 MB (CPU; GPU variants larger)
- `insightface`: ~5–10 MB (core); models separate
- `opencv-python`: ~20–30 MB
- **Total ML deps:** ~75–120 MB (without models)

**ONNX Models:**
- Model files stored separately (not pip-installed)
- Typical face model: 300–500 MB (e.g., insightface .onnx weights)
- Bundle path: `sidecar_dir/models/` or pass via command-line arg

## 4. Bundle Size Estimates

| Component | Size | Notes |
|-----------|------|-------|
| Python 3.11 runtime | 100–150 MB | glibc/Windows/macOS variants differ |
| venv + pip overhead | 5–10 MB | Minimal if clean |
| ML dependencies (onnxruntime, insightface, opencv) | 75–120 MB | CPU-only; GPU doubles this |
| ONNX model files | 300–500 MB | Per-model; exclude if not pre-shipped |
| **Total (no models pre-shipped)** | **~280–400 MB** | Per platform |
| **Total (1 model included)** | **~580–900 MB** | Payload balloons fast |

**Optimization:** Ship Python + deps; let users download models on first run or via auto-updater.

## 5. Alternatives Assessment

### PyInstaller One-Shot Bundle
**Pros:**
- Single executable; no setup
- Transparent dependency collection

**Cons:**
- Larger: 33–90 MB+ for basic PyQt6 apps; ML apps easily 500+ MB
- UPX compression available but slow startup
- Harder to update Python/deps post-deploy
- Slower execution (unpacking overhead)

**Verdict:** Overkill for Python-as-backend; better for pure-Python CLI tools.

### Python-Build-Standalone + venv (Recommended)
**Pros:**
- Minimal; only ~100 MB Python core
- Clean dependency isolation (venv)
- Easy post-deployment updates (pip upgrade in venv)
- Works natively on all platforms
- Standard Python tooling (pip, venv)

**Cons:**
- Slightly more setup during app startup (extract, validate paths)
- Manual platform-specific binary selection

**Verdict:** Best fit for Tauri; portable, updatable, low friction.

---

## Unresolved Questions

1. **Model distribution:** Pre-ship ONNX model files or download on first run?
2. **PATH env config:** How does standalone Python locate system libs (e.g., `libssl.so.3`)?
3. **pip wheel caching:** Best practice for offline venv creation in installer?
4. **Minimum Python version:** Does 3.11 meet all ML deps, or upgrade to 3.12/3.13?

# Phase 2: Bundle python-build-standalone as Tauri Sidecar

**Effort:** 4h
**Team:** Team 2 (Backend)
**Depends on:** Phase 1 (needs working server.py with all endpoints)

---

## Problem

Users must install Python 3.11+, pip, venv, and all ML dependencies manually before the app works. The current Tauri sidecar config (`externalBin: ["binaries/deep-live-cam-server"]`) expects a pre-built binary that does not yet exist.

## Goal

Ship a self-contained Python runtime + venv + all dependencies as a Tauri sidecar binary. Zero user-side Python installation required.

## Strategy: python-build-standalone + venv (Not PyInstaller)

Per research (`researcher-01-python-bundle.md`), python-build-standalone + venv is preferred over PyInstaller:

| Criterion | python-build-standalone + venv | PyInstaller |
|-----------|-------------------------------|-------------|
| Bundle size | ~280-400 MB (no models) | 500+ MB |
| Startup time | Native Python speed | Slow (unpacking) |
| Post-deploy updates | `pip install --upgrade` in venv | Full rebuild |
| Debugging | Standard Python stack | Obfuscated paths |
| Platform builds | Download per arch | Build per arch |

## Architecture

```
src-tauri/
  binaries/
    deep-live-cam-server-x86_64-unknown-linux-gnu    (shell wrapper)
    deep-live-cam-server-x86_64-pc-windows-msvc.exe  (batch wrapper)
    deep-live-cam-server-aarch64-apple-darwin         (shell wrapper)
  sidecar/
    python/                    <- python-build-standalone (per-platform)
      bin/python3              (linux/mac)
      python.exe               (windows)
    venv/                      <- created by build script
      lib/python3.11/site-packages/
        cv2, onnxruntime, insightface, fastapi, uvicorn, ...
    app/                       <- Deep-Live-Cam source code
      server.py
      modules/
    models/                    <- ONNX model files (downloaded at first run)
```

### Wrapper Script Pattern

Tauri's `externalBin` expects a single executable per platform. The wrapper script bootstraps the venv Python and runs `server.py`:

**Linux/macOS (`deep-live-cam-server-{triple}`):**
```bash
#!/bin/bash
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SIDECAR_DIR="$SCRIPT_DIR/../sidecar"
export PYTHONHOME="$SIDECAR_DIR/python"
exec "$SIDECAR_DIR/venv/bin/python" "$SIDECAR_DIR/app/server.py" "$@"
```

**Windows (`deep-live-cam-server-x86_64-pc-windows-msvc.exe`):**
Option A: Batch file renamed to .exe (won't work -- Tauri needs real EXE).
Option B: Compile a tiny Rust/C launcher that calls `python.exe server.py`. Simpler: use a `.cmd` wrapper and configure Tauri shell scope to allow it.
**Recommended:** Small Rust binary (~50 lines) in `src-tauri/sidecar-launcher/` that resolves paths and spawns Python.

---

## Implementation Steps

### Step 1: Create build script `scripts/build-sidecar.sh`

Downloads python-build-standalone, creates venv, installs deps, copies app source.

```bash
#!/bin/bash
set -euo pipefail

PYTHON_VERSION="3.11.11"
PBS_RELEASE="20260127"
PLATFORM=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

# Map to python-build-standalone naming
case "$PLATFORM-$ARCH" in
  linux-x86_64)   TRIPLE="x86_64-unknown-linux-gnu" ; EXT="tar.zst" ;;
  darwin-arm64)   TRIPLE="aarch64-apple-darwin"      ; EXT="tar.zst" ;;
  darwin-x86_64)  TRIPLE="x86_64-apple-darwin"       ; EXT="tar.zst" ;;
  *)              echo "Unsupported: $PLATFORM-$ARCH"; exit 1 ;;
esac

URL="https://github.com/astral-sh/python-build-standalone/releases/download/${PBS_RELEASE}/cpython-${PYTHON_VERSION}+${PBS_RELEASE}-${TRIPLE}-install_only_stripped.${EXT}"

SIDECAR_DIR="src-tauri/sidecar"
mkdir -p "$SIDECAR_DIR"

# 1. Download and extract Python
echo "[BUILD] Downloading python-build-standalone..."
curl -fSL "$URL" | tar --zstd -xf - -C "$SIDECAR_DIR"
mv "$SIDECAR_DIR/python" "$SIDECAR_DIR/python" 2>/dev/null || true

# 2. Create venv
echo "[BUILD] Creating venv..."
"$SIDECAR_DIR/python/bin/python3" -m venv "$SIDECAR_DIR/venv"

# 3. Install dependencies (server-only subset of requirements.txt)
echo "[BUILD] Installing dependencies..."
"$SIDECAR_DIR/venv/bin/pip" install --no-cache-dir \
  numpy "opencv-python==4.10.0.84" "insightface==0.7.3" \
  "onnx==1.18.0" "onnxruntime-gpu==1.24.2" \
  fastapi uvicorn[standard] python-multipart \
  "psutil==5.9.8" "protobuf==4.25.1"

# 4. Copy application source
echo "[BUILD] Copying app source..."
mkdir -p "$SIDECAR_DIR/app"
cp Deep-Live-Cam/server.py "$SIDECAR_DIR/app/"
cp -r Deep-Live-Cam/modules "$SIDECAR_DIR/app/"

# 5. Create models directory (populated at first run)
mkdir -p "$SIDECAR_DIR/models"

# 6. Create wrapper script
TAURI_TRIPLE=$(rustc -vV | grep 'host:' | awk '{print $2}')
WRAPPER="src-tauri/binaries/deep-live-cam-server-${TAURI_TRIPLE}"
cat > "$WRAPPER" << 'WRAPPER_EOF'
#!/bin/bash
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SIDECAR="$SCRIPT_DIR/../sidecar"
export PYTHONHOME="$SIDECAR/python"
export PYTHONPATH="$SIDECAR/app"
exec "$SIDECAR/venv/bin/python" "$SIDECAR/app/server.py" "$@"
WRAPPER_EOF
chmod +x "$WRAPPER"

echo "[BUILD] Sidecar built for $TRIPLE"
```

**CCS Delegation:** Team 2 Leader writes initial version. mmu agents handle per-platform variants.

### Step 2: Create Windows build script `scripts/build-sidecar-win.ps1`

PowerShell equivalent: downloads Windows python-build-standalone, creates venv, installs deps.

Key differences from Unix:
- URL uses `x86_64-pc-windows-msvc-install_only.tar.gz`
- `python.exe -m venv` instead of `python3 -m venv`
- Wrapper is a compiled Rust launcher (see Step 4)

**CCS Delegation:** mmu agent -- mechanical translation of the bash script.

### Step 3: Create macOS build script `scripts/build-sidecar-macos.sh`

Key differences from Linux:
- macOS uses `onnxruntime-silicon` instead of `onnxruntime-gpu` on ARM64
- Apple Silicon triple: `aarch64-apple-darwin`
- Intel Mac triple: `x86_64-apple-darwin`
- May need `DYLD_LIBRARY_PATH` in wrapper for OpenCV

**CCS Delegation:** mmu agent.

### Step 4: Create Windows sidecar launcher (Rust)

Tiny Rust program in `src-tauri/sidecar-launcher/`:

```rust
// src-tauri/sidecar-launcher/src/main.rs
use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let exe_dir = env::current_exe()
        .expect("cannot get exe path")
        .parent().unwrap().to_path_buf();

    let sidecar = exe_dir.join("..").join("sidecar");
    let python = sidecar.join("venv").join("Scripts").join("python.exe");
    let server = sidecar.join("app").join("server.py");

    let status = Command::new(python)
        .env("PYTHONHOME", sidecar.join("python"))
        .env("PYTHONPATH", sidecar.join("app"))
        .arg(&server)
        .args(env::args().skip(1))
        .status()
        .expect("failed to start Python");

    std::process::exit(status.code().unwrap_or(1));
}
```

Build with: `cargo build --release --manifest-path src-tauri/sidecar-launcher/Cargo.toml`
Copy output to: `src-tauri/binaries/deep-live-cam-server-x86_64-pc-windows-msvc.exe`

**CCS Delegation:** mmu agent -- boilerplate Rust.

### Step 5: Update `tauri.conf.json` for sidecar resources

The sidecar directory must be included in the bundle. Add to `tauri.conf.json`:

```json
{
  "bundle": {
    "resources": [
      "sidecar/**/*"
    ]
  }
}
```

This ensures Tauri copies the entire sidecar directory into the app bundle at build time.

**CCS Delegation:** mmu agent.

### Step 6: Update `main.rs` sidecar path resolution

Current code: `app.shell().sidecar("binaries/deep-live-cam-server")`

This should continue to work -- Tauri auto-appends the target triple and resolves relative to the bundle. Verify the wrapper scripts are at the expected paths.

**CCS Delegation:** Team 2 Leader verifies.

### Step 7: Model download on first run

Add a startup check in `server.py` (or a separate `setup.py` script):

```python
REQUIRED_MODELS = {
    "inswapper_128_fp16.onnx": "https://huggingface.co/...",
    "gfpgan-1024.onnx": "https://huggingface.co/...",
}

def check_and_download_models(models_dir: str):
    for filename, url in REQUIRED_MODELS.items():
        path = os.path.join(models_dir, filename)
        if not os.path.exists(path):
            print(f"[SERVER] Downloading {filename}...")
            # Download with progress (use urllib or httpx)
```

**Decision:** Download on first run, not bundled. Models are 300-500MB each. Keeps installer under 400MB.

**CCS Delegation:** mmu agent for the download logic. Team 2 Leader defines model URLs and checksums.

### Step 8: Create `.gitignore` entries

```
src-tauri/sidecar/
src-tauri/binaries/deep-live-cam-server-*
```

The sidecar is a build artifact, not source code.

**CCS Delegation:** mmu agent.

---

## Size Budget

| Component | Estimated Size | Notes |
|-----------|---------------|-------|
| Python 3.11 (stripped) | 40-60 MB | `install_only_stripped` variant |
| venv + pip metadata | 5 MB | |
| onnxruntime-gpu | 80 MB | CPU-only: ~50 MB |
| opencv-python | 25 MB | Headless variant is 15 MB |
| insightface | 8 MB | Core only, models separate |
| fastapi + uvicorn | 5 MB | |
| numpy + other deps | 30 MB | |
| App source code | 1 MB | |
| **Total (no models)** | **~200-250 MB** | Compressed in installer: ~120-150 MB |
| ONNX models (downloaded) | 300-500 MB | Not in installer |

## Optimization Options (v2, not for this sprint)

- Use `opencv-python-headless` to save 10 MB
- Strip unused stdlib modules (`tkinter`, `test`, `idlelib`) to save 20 MB
- Use `onnxruntime` (CPU) instead of `onnxruntime-gpu` for smaller default, offer GPU as optional download
- Use `zstd` compression for the sidecar directory in the installer

---

## CCS Delegation Map

| Step | Task | Assignee | Rationale |
|------|------|----------|-----------|
| 1 | Linux build script | **Team 2 Leader** | Core deliverable, sets pattern |
| 2 | Windows build script | mmu | Mechanical translation |
| 3 | macOS build script | mmu | Mechanical translation |
| 4 | Windows Rust launcher | mmu | Boilerplate Rust |
| 5 | Update tauri.conf.json | mmu | Config change |
| 6 | Verify main.rs paths | **Team 2 Leader** | Integration verification |
| 7 | Model download logic | mmu (download) + **Leader** (URLs/checksums) | Split responsibility |
| 8 | .gitignore entries | mmu | Trivial |

---

## Success Criteria

- [ ] `scripts/build-sidecar.sh` runs on Linux and produces working sidecar in `src-tauri/sidecar/`
- [ ] `pnpm tauri dev` starts the app, sidecar spawns, `GET /health` returns 200
- [ ] No system Python required -- removing `python3` from PATH does not break the app
- [ ] Camera and enhancer endpoints from Phase 1 work through the bundled sidecar
- [ ] Models directory is created; if models missing, server logs download URLs
- [ ] Bundle size (sidecar directory) is under 300 MB uncompressed

## Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| python-build-standalone binary incompatible with system glibc | Crash on older Linux | Target glibc 2.17+ (matches PBS builds); document minimum OS versions |
| `onnxruntime-gpu` requires CUDA runtime | Fails on machines without NVIDIA GPU | Fallback: `onnxruntime` (CPU) if CUDA not detected at startup |
| venv creation fails inside standalone Python | Build script fails | Test with exact PBS release version; pin to `20260127` |
| Wrapper script path resolution breaks on spaces in install path | Sidecar won't start | Quote all paths in wrapper; test with "Program Files" path on Windows |
| macOS Gatekeeper blocks unsigned sidecar binary | App won't launch | Handle in Phase 3 (code signing); for dev: `xattr -cr` workaround |

## Unresolved Questions

1. Should the Windows launcher be a compiled Rust binary or a `.cmd` batch file? (Rust is cleaner but adds a build step)
2. Which Hugging Face URLs to use for model downloads? Need to confirm licensing for redistribution.
3. Should `onnxruntime-silicon` be the default on macOS ARM64, or should we ship `onnxruntime` (CPU) for simplicity?

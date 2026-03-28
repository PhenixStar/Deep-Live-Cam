#!/bin/bash
# Build portable Python sidecar for macOS (ARM64 + Intel)
set -euo pipefail

PYTHON_VERSION="3.11.11"
PBS_RELEASE="20250317"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SIDECAR_DIR="$REPO_ROOT/app/src-tauri/sidecar"
BINARIES_DIR="$REPO_ROOT/app/src-tauri/binaries"

# Detect architecture
ARCH=$(uname -m)
case "$ARCH" in
  arm64)  TRIPLE="aarch64-apple-darwin" ;;
  x86_64) TRIPLE="x86_64-apple-darwin" ;;
  *)      echo "[ERROR] Unsupported arch: $ARCH"; exit 1 ;;
esac

URL="https://github.com/astral-sh/python-build-standalone/releases/download/${PBS_RELEASE}/cpython-${PYTHON_VERSION}+${PBS_RELEASE}-${TRIPLE}-install_only_stripped.tar.gz"

echo "[BUILD] Target: macOS $TRIPLE"
echo "[BUILD] Python: $PYTHON_VERSION (PBS $PBS_RELEASE)"

# Clean previous build
rm -rf "$SIDECAR_DIR"
mkdir -p "$SIDECAR_DIR" "$BINARIES_DIR"

# 1. Download and extract Python
echo "[BUILD] Downloading python-build-standalone..."
curl -fSL "$URL" | tar -xzf - -C "$SIDECAR_DIR"

# 2. Create virtual environment
echo "[BUILD] Creating virtual environment..."
"$SIDECAR_DIR/python/bin/python3" -m venv "$SIDECAR_DIR/venv"

# 3. Install dependencies (CPU-only onnxruntime on macOS, no CUDA)
echo "[BUILD] Installing Python dependencies..."
"$SIDECAR_DIR/venv/bin/pip" install --no-cache-dir \
  numpy "opencv-python==4.10.0.84" "insightface==0.7.3" \
  onnxruntime \
  fastapi "uvicorn[standard]" python-multipart \
  "psutil==5.9.8" "protobuf==4.25.1"

# 4. Copy application source
echo "[BUILD] Copying app source..."
mkdir -p "$SIDECAR_DIR/app"
cp "$REPO_ROOT/core/server.py" "$SIDECAR_DIR/app/"
cp -r "$REPO_ROOT/core/modules" "$SIDECAR_DIR/app/"

# 5. Create models directory (populated at first run)
mkdir -p "$SIDECAR_DIR/models"

# 6. Create wrapper script
WRAPPER="$BINARIES_DIR/deep-live-cam-server-${TRIPLE}"
cat > "$WRAPPER" << 'WRAPPER_EOF'
#!/bin/bash
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SIDECAR="$SCRIPT_DIR/../sidecar"
export PYTHONHOME="$SIDECAR/python"
export PYTHONPATH="$SIDECAR/app"
export DEEP_LIVE_CAM_MODELS_DIR="$SIDECAR/models"
# Note: DYLD_LIBRARY_PATH is stripped by macOS SIP for hardened binaries.
# python-build-standalone bundles its own dylibs, so this is not needed.
exec "$SIDECAR/venv/bin/python" "$SIDECAR/app/server.py" "$@"
WRAPPER_EOF
chmod +x "$WRAPPER"

echo "[BUILD] Sidecar built successfully for $TRIPLE"
echo "[BUILD] Size: $(du -sh "$SIDECAR_DIR" | cut -f1)"

#!/bin/bash
set -e
cd "$(dirname "$0")"

# Install Python 3.11 if missing
command -v python3.11 >/dev/null || {
  sudo add-apt-repository -y ppa:deadsnakes/ppa
  sudo apt update && sudo apt install -y python3.11 python3.11-venv python3.11-dev python3.11-tk
}

# Create venv
[ -d .venv ] || python3.11 -m venv .venv
source .venv/bin/activate

pip install --upgrade pip setuptools wheel
pip install Cython numpy==1.26.4
pip install nvidia-cudnn-cu12==9.1.0.70 nvidia-cublas-cu12
REQ_TMP=$(mktemp /tmp/req-linux.XXXXXX)
grep -v pygrabber requirements.txt > "$REQ_TMP"
pip install -r "$REQ_TMP"
rm -f "$REQ_TMP"
pip install -U torch torchvision torchaudio --index-url https://download.pytorch.org/whl/cu121
pip install huggingface-hub

# Configure cuDNN
CUDNN_PATH=$(python -c "import nvidia.cudnn; print(nvidia.cudnn.__path__[0])")
grep -q "LD_LIBRARY_PATH.*cudnn" .venv/bin/activate || \
  echo "export LD_LIBRARY_PATH=${CUDNN_PATH}/lib:\$LD_LIBRARY_PATH" >> .venv/bin/activate

# Download models (respect DEEP_LIVE_CAM_MODELS_DIR if set)
MODELS_DIR="${DEEP_LIVE_CAM_MODELS_DIR:-$(pwd)/models}"
mkdir -p "$MODELS_DIR"
huggingface-cli download hacksider/deep-live-cam \
  --local-dir "$MODELS_DIR" --local-dir-use-symlinks False
[ -L "$MODELS_DIR/inswapper_128.onnx" ] || ln -sf inswapper_128_fp16.onnx "$MODELS_DIR/inswapper_128.onnx"

echo "Setup complete. Run: ./start.sh"

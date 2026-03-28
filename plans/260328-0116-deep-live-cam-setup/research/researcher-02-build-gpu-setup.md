# Deep-Live-Cam: Build Process & GPU Acceleration Research

**Date:** 2026-03-28 | **Platform:** Linux 6.8.0-106-generic with NVIDIA GPU

## 1. Installation Process (Linux)

```bash
# 1. Clone repo
git clone https://github.com/hacksider/Deep-Live-Cam.git
cd Deep-Live-Cam

# 2. Create venv (required, avoids dep conflicts)
python3.11 -m venv venv
source venv/bin/activate

# 3. Install deps from requirements.txt
pip install -r requirements.txt

# 4. Verify FFmpeg installed
ffmpeg -version
```

Prerequisites: Python 3.11, pip, git, FFmpeg, Visual Studio 2022 Runtimes (Windows only).

## 2. GPU Acceleration Setup (NVIDIA)

**CUDA Requirements:**
- CUDA Toolkit: 12.8.0 (from docs) or compatible
- cuDNN: v8.9.7
- onnxruntime-gpu: 1.24.2 (from requirements.txt)

**Installation:**
```bash
# onnxruntime-gpu automatically pulled by pip install
# Verify GPU detection:
python3 -c "import onnxruntime; print(onnxruntime.get_available_providers())"
# Should show: ['CUDAExecutionProvider', 'CPUExecutionProvider']
```

Alternative GPU providers (requirements.txt conditional):
- Apple Silicon: onnxruntime-silicon 1.16.3
- Non-NVIDIA AMD/Intel (Windows): onnxruntime-directml
- Intel: onnxruntime-openvino

## 3. Local Model Setup

**Directory Structure:**
```
/raid/models/
├── vision/
│   ├── face-recognition/yolov8s-face-lindevs.onnx (available)
│   └── sd3.5-large/ (TBD)
├── llm/ (available)
└── embedding/ (available)
```

**Required Models for Deep-Live-Cam:**
1. **Face Swapper**: `inswapper_128_fp16.onnx` (~300MB) - NOT found locally
2. **Face Enhancer**: `GFPGANv1.4.pth` (~350MB) - NOT found locally
3. **Face Detector**: InsightFace built-in models (~200MB)

**Configuration:**
- Edit app config to point to `/raid/models/`
- Models auto-download to `models/` dir on first run if missing
- For offline use: manually place models in project `models/` directory before running

## 4. FFmpeg Requirements

- **Minimum**: version with libx264/libx265 (video encoding)
- **Features needed**:
  - Video codec support (H.264, HEVC)
  - Audio stream handling
  - Format conversion
- **Installation**: `sudo apt install ffmpeg` (Ubuntu/Debian)
- **Verify**: `ffmpeg -codecs | grep -i h264`

## 5. Face Swap Pipeline (Technical)

```
Input Video/Webcam
    ↓
[InsightFace] Face Detection (YOLOv8-based)
    ↓ (returns bounding boxes + landmarks)
[GFPGAN/CodeFormer] Face Enhancement
    ↓ (improves quality, removes noise)
[ONNX Face Swapper] (inswapper_128_fp16)
    ↓ (latent space face replacement)
Frame Reconstruction
    ↓
Output Video/Stream
```

**Key Components:**
- **Detection**: InsightFace (face location + 68-point landmarks)
- **Enhancement**: GFPGAN (v1.4) or CodeFormer (temporal consistency)
- **Swap Engine**: ONNX Runtime face swapper (lightweight, GPU-optimized)
- **Mouthmasking**: Optional post-process to preserve mouth movements

## 6. Performance Optimization

**GPU Memory Management:**
- Batch size: Start with 1, increase if VRAM >8GB
- Resolution: 512×512 default (reduce to 384×384 for 6GB VRAM)
- FP16 mode: Use `inswapper_128_fp16.onnx` instead of FP32 (2x memory savings)

**Latency Reduction:**
- Model caching: Loaded once at startup
- Async frame queue: Enable if available
- Skip frames: Process every Nth frame for >30fps targets
- Resolution downsampling: Process at 480p, upscale output

**Recommended Settings (8GB+ GPU):**
```
resolution: 512×512
batch_size: 1
enhancement: GFPGANv1.4 (faster than CodeFormer)
dtype: FP16
```

## 7. Key Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| onnxruntime-gpu | 1.24.2 | ONNX inference engine |
| insightface | 0.7.3 | Face detection/landmarks |
| opencv-python | 4.10.0.84 | Video I/O, image ops |
| numpy | 1.23.5-<2 | Numerical computing |
| pillow | 12.1.1 | Image processing |
| customtkinter | 5.2.2 | GUI framework |

## Open Questions

1. **FP16 vs FP32 trade-off**: Are accuracy losses acceptable at FP16 on user photos?
2. **CodeFormer availability**: Is CodeFormer model bundled or requires separate download?
3. **Model URLs**: Where are missing models (inswapper, GFPGAN) sourced from HuggingFace?
4. **Streaming latency**: Expected end-to-end latency for 1080p @ 30fps with GPU?

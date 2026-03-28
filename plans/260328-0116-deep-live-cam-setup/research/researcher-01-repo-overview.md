# Deep-Live-Cam Repository Research

## Project Overview
**Purpose:** Real-time face-swapping application using single reference image to swap faces in videos and live webcam feeds. Built on Python with GPU acceleration support. Marketed as productivity tool for AI-generated media (character animation, content creation, fashion design). Includes content filters and ethical safeguards.

**Key Features:**
- Live webcam streaming with face replacement
- Video file processing with multi-face support
- Mouth masking to preserve original mouth movements
- GUI and CLI interfaces
- Cross-platform (Windows, macOS, Linux)

---

## Python & Core Dependencies
- **Python:** 3.10+ (3.11 recommended; 3.10 mandatory for macOS Apple Silicon)
- **Key Packages:**
  - torch, torchvision, torchaudio (PyTorch)
  - onnxruntime (or GPU-specific: onnxruntime-gpu 1.24.2, onnxruntime-silicon 1.16.3)
  - insightface 0.7.3 (face detection/analysis)
  - opencv-python 4.10.0.84
  - customtkinter 5.2.2 (GUI)
  - pillow 12.1.1, numpy <2, protobuf 4.25.1
  - opennsfw2 0.10.2 (content filter)
  - GFPGAN, BasicSR (face enhancement)

---

## GPU Requirements & Support
**NVIDIA (CUDA):**
- CUDA Toolkit 12.8.0, cuDNN v8.9.7
- PyTorch install: `pip install -U torch torchvision torchaudio --index-url https://download.pytorch.org/whl/cu128`
- onnxruntime-gpu 1.24.2 or 1.21.0 (version conflicts reported)

**Alternative Providers:**
- DirectML (Windows): onnxruntime-directml 1.21.0
- CoreML (Apple Silicon): onnxruntime-silicon 1.13.1 or 1.16.3
- OpenVINO (Intel): onnxruntime-openvino 1.21.0

---

## Required Model Files
**Models needed (from HuggingFace):**
1. `GFPGANv1.4.onnx` - Face enhancement
2. `inswapper_128_fp16.onnx` - Face swapping engine

**Placement:** models/ folder in project directory

**Note:** File sizes and exact download URLs not specified in README; exact paths and version pinning missing—common source of "model not found" errors.

---

## Known Issues & Limitations
**Critical Setup Problems:**
- Python version contradictions in README (3.10 vs 3.11)
- Dependency build failures (insightface wheel building)
- PyTorch/ONNXRuntime version conflicts on newer GPUs (RTX 5080, M3 Mac)
- Models pointing to incorrect file paths; inconsistent filename references in code

**Runtime Errors:**
- Model loading failures with NoneType exceptions
- Face detection errors ("full_face_poly not defined")
- Blank/frozen camera preview or stuck at 99%
- Visual artifacts (green rectangles, black boxes over faces)
- Mouth mask activation causing crashes
- Missing output files after video processing

**Repository Status:** 75+ open issues, limited recent maintenance activity.

---

## Environment & Setup for This Project
- **OS:** Linux (Ubuntu 6.8.0+)
- **GPU:** NVIDIA with CUDA support ✓
- **Model Storage:** /raid/models/ (available)
- **Status:** Ready for installation validation & dependency resolution testing

---

## Unresolved Questions
1. Exact download URLs for model files from HuggingFace
2. File sizes for GFPGANv1.4.onnx and inswapper_128_fp16.onnx
3. Which version of onnxruntime-gpu (1.21.0 vs 1.24.2) is most stable
4. Why model path references are inconsistent in code
5. Whether tfmpeg must be system-installed or pip-installable

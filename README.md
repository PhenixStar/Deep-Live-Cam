# Deep Forge

Real-time face swap desktop app with a single click and only a single image. Built with Rust + Tauri for native performance across Windows, Linux, and macOS.

## Features

- **Camera Selection** -- pick which webcam to use from a dropdown
- **Face Enhancers** -- toggle GFPGAN, GPEN-256, GPEN-512 on/off in real-time
- **GPU Acceleration** -- CUDA (NVIDIA), DirectML (AMD/Intel), or CPU fallback
- **Auto-Update** -- checks for new versions on startup
- **Cross-Platform** -- Windows MSI, Linux AppImage/deb, macOS DMG

## Installation

### Download (Recommended)

Grab the latest installer from [Releases](https://github.com/PhenixStar/Deep-Forge/releases):

| Platform | File |
|----------|------|
| Windows | `Deep-Forge_x.x.x_x64-setup.exe` or `.msi` |
| Linux | `.AppImage` or `.deb` |
| macOS (ARM) | `.dmg` |

### Build from Source

**Prerequisites:** Rust, Node.js 22+, pnpm 10+

```bash
git clone https://github.com/PhenixStar/Deep-Forge.git
cd Deep-Forge

# Build the Rust sidecar backend
# Linux/macOS:
bash scripts/build-rust-sidecar.sh
# Windows (PowerShell):
pwsh scripts/build-rust-sidecar-win.ps1

# Install frontend deps and run
cd app
pnpm install
pnpm tauri dev
```

For a release build:
```bash
cd app
pnpm tauri build
```

## Architecture

```
Deep-Forge/
├── core/                    # Backend
│   ├── rust-engine/         # Rust inference server (v0.2+)
│   │   ├── dlc-core/        #   Face detection, swap, enhancement (ort + ONNX)
│   │   ├── dlc-capture/     #   Camera capture (opencv feature flag)
│   │   └── dlc-server/      #   axum HTTP/WS server on :8008
│   ├── server.py            # Python server (v0.1 legacy)
│   └── modules/             # Python inference modules
├── app/                     # Tauri desktop app
│   ├── src/                 #   React frontend
│   └── src-tauri/           #   Rust shell (sidecar management, auto-update)
├── scripts/                 # Build helpers
└── .github/workflows/       # CI/CD (release + PR checks)
```

## GPU Support

| GPU Vendor | Provider | Notes |
|------------|----------|-------|
| NVIDIA | CUDA | Best performance on discrete GPUs |
| AMD | DirectML | Works on Radeon iGPUs (890M etc.) and discrete |
| Intel | DirectML | Arc GPUs and integrated graphics |
| CPU | CPUExecutionProvider | Fallback, works everywhere |

Auto-detected at startup. No configuration needed.

## API Endpoints

The backend server runs on `http://localhost:8008`:

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Server status |
| GET | `/cameras` | List available cameras |
| POST | `/camera/{index}` | Switch active camera |
| GET | `/settings` | Current enhancer toggles |
| POST | `/settings` | Toggle face enhancers |
| POST | `/source` | Upload source face image |
| POST | `/swap/image` | Face swap (source + target images) |
| WS | `/ws/video` | Live video stream with face swap |

## Models

ONNX models are downloaded on first run to the `models/` directory:

- `det_10g.onnx` -- SCRFD face detection (buffalo_l)
- `w600k_r50.onnx` -- ArcFace embedding extraction
- `inswapper_128.onnx` -- Face swap
- `gfpgan-1024.onnx` -- GFPGAN face enhancement
- `GPEN-BFR-256.onnx` / `GPEN-BFR-512.onnx` -- GPEN enhancement

## Development

```bash
# Run Rust tests (24 tests)
cd core/rust-engine && cargo test

# Run Python server (legacy)
cd core && python server.py

# Frontend dev
cd app && pnpm dev

# Full Tauri dev (builds sidecar + launches app)
cd app && pnpm tauri dev
```

## License

AGPL-3.0. Based on [Deep-Live-Cam](https://github.com/hacksider/Deep-Live-Cam).

## Disclaimer

This software is intended for legitimate creative and research use. Users must obtain consent when using real persons' likenesses and clearly label deepfake content. The built-in content filter blocks inappropriate media. By using this software, you agree to use it responsibly and in compliance with applicable laws.

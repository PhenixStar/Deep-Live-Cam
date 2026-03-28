# Phase 3: Tauri Desktop App (Approach A)

**Workstream:** WS3
**Owner:** Team 3 Leader
**Timeline:** Weeks 1-6 (parallel with WS2)
**Agents:** 5 CCS-delegated

---

## Objective

Wrap the existing Python Deep-Live-Cam backend in a Tauri v2 desktop app with modern web UI, WebSocket frame streaming, platform installers, and auto-update. Ship as an installable app (MSI/DMG/AppImage) with bundled Python runtime.

## Architecture

```
┌─────────────────────────────────────────────────┐
│  Tauri v2 App                                   │
│  ┌──────────────────────────────────────────┐   │
│  │  Web Frontend (React + Tailwind)         │   │
│  │  ├── VideoCanvas (WebSocket binary)      │   │
│  │  ├── ControlPanel (face-swap settings)   │   │
│  │  ├── FileSelector (source/target)        │   │
│  │  └── StatusBar (FPS, GPU, model)         │   │
│  └──────────────────┬───────────────────────┘   │
│                     │ WebSocket / HTTP           │
│  ┌──────────────────▼───────────────────────┐   │
│  │  Python Sidecar (FastAPI + uvicorn)      │   │
│  │  ├── /swap/image    (POST, multipart)    │   │
│  │  ├── /swap/live     (WS, binary frames)  │   │
│  │  ├── /config        (POST, JSON)         │   │
│  │  ├── /health        (GET)                │   │
│  │  └── Deep-Live-Cam modules (unchanged)   │   │
│  └──────────────────────────────────────────┘   │
│  System tray | Auto-update | Native file dialog │
└─────────────────────────────────────────────────┘
```

---

## Steps

### Week 1: Tauri Scaffold + Sidecar Config

**CCS Agents:**
- Claude (Agent 1): Tauri config, security capabilities, sidecar architecture
- mmhs (Agent 3): FastAPI server skeleton

#### 1.1 Initialize Tauri v2 project

```bash
npm create tauri-app@latest deep-live-cam-app -- \
  --template react-ts \
  --manager npm
cd deep-live-cam-app
npm install
```

#### 1.2 Configure sidecar in `tauri.conf.json`

```json
{
  "bundle": {
    "externalBin": [
      "bin/dlc-server-x86_64-unknown-linux-gnu",
      "bin/dlc-server-x86_64-pc-windows-msvc",
      "bin/dlc-server-aarch64-apple-darwin"
    ]
  }
}
```

#### 1.3 Security capabilities (`capabilities/default.json`)

```json
{
  "identifier": "default",
  "description": "Default capabilities for Deep-Live-Cam",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "shell:allow-spawn",
    "shell:allow-execute",
    "dialog:allow-open",
    "dialog:allow-save",
    "fs:allow-read",
    "fs:allow-write"
  ]
}
```

#### 1.4 FastAPI server skeleton

```python
# src/backends/main.py
from fastapi import FastAPI, WebSocket
from fastapi.middleware.cors import CORSMiddleware
import uvicorn

app = FastAPI()
app.add_middleware(CORSMiddleware, allow_origins=["*"])

@app.get("/health")
async def health():
    return {"status": "ok", "gpu": detect_gpu(), "model": get_model_name()}

@app.post("/swap/image")
async def swap_image(source: UploadFile, target: UploadFile):
    # Existing face_swapper.process_frame() logic
    pass

@app.websocket("/swap/live")
async def swap_live(ws: WebSocket):
    await ws.accept()
    # Binary frame streaming loop
    pass

@app.post("/config")
async def update_config(config: SwapConfig):
    # Update globals (execution provider, model, options)
    pass

if __name__ == "__main__":
    uvicorn.run(app, host="127.0.0.1", port=8008)
```

**Delegation:** Claude designs API contract (shared with WS2 Rust team). mmhs implements FastAPI server wrapping existing modules.

---

### Week 2: Python Sidecar Bundling

**CCS Agents:**
- mmhs (Agent 4): Build scripts for python-build-standalone + PyInstaller

#### 2.1 Download python-build-standalone

```bash
# scripts/build-sidecar-linux.sh
PBS_VERSION="20260301"
PBS_URL="https://github.com/indygreg/python-build-standalone/releases/download/${PBS_VERSION}/cpython-3.12-x86_64-unknown-linux-gnu-install_only.tar.gz"
curl -L "$PBS_URL" -o /tmp/python-standalone.tar.gz
tar xzf /tmp/python-standalone.tar.gz -C /tmp/python-standalone/
```

#### 2.2 Install dependencies into standalone Python

```bash
/tmp/python-standalone/bin/python3 -m pip install \
  -r requirements.txt \
  fastapi uvicorn[standard] websockets
```

#### 2.3 Build with PyInstaller

```bash
/tmp/python-standalone/bin/python3 -m PyInstaller \
  --onefile \
  --name dlc-server \
  --hidden-import=onnxruntime \
  --collect-all onnxruntime \
  --add-data "modules:modules" \
  src/backends/main.py
```

#### 2.4 Copy binary to Tauri bin directory

```bash
cp dist/dlc-server src-tauri/bin/dlc-server-x86_64-unknown-linux-gnu
```

**Per-platform scripts needed:** Linux (x86_64), Windows (x86_64), macOS (aarch64).

---

### Week 3: Web UI

**CCS Agents:**
- mm (Agent 2): React components
- Claude (Agent 1): WebSocket streaming integration

#### 3.1 UI Component Tree

```
App.tsx
├── Header (app title, connection status indicator)
├── TabView
│   ├── ImageTab
│   │   ├── ImageDropzone (source face)
│   │   ├── ImageDropzone (target image)
│   │   ├── ProcessButton
│   │   └── ResultViewer
│   ├── VideoTab
│   │   ├── FileSelector (source video)
│   │   ├── FileSelector (target face)
│   │   ├── ProgressBar
│   │   └── ResultViewer
│   ├── LiveTab
│   │   ├── CameraSelector (dropdown)
│   │   ├── VideoCanvas (WebSocket frames)
│   │   ├── FaceSelector (source face picker)
│   │   └── ControlBar (start/stop, FPS display)
│   └── SettingsTab
│       ├── ExecutionProvider (CPU/CUDA/CoreML/DirectML)
│       ├── ModelSelector (inswapper variants)
│       ├── QualitySlider (JPEG quality)
│       └── AdvancedOptions (threads, memory limit)
├── StatusBar (FPS, GPU name, model loaded, latency)
└── SystemTray (minimize to tray)
```

**Delegation:** mm builds all components from wireframe. Claude integrates WebSocket binary streaming.

#### 3.2 Feature parity with CustomTkinter UI

Must support all current UI features:
- Source/target face selection
- Multi-face mapping (`--map-faces`)
- Mouth mask toggle
- Execution provider selection
- Frame processor pipeline config
- Batch progress reporting

---

### Week 4: WebSocket Binary Frame Streaming

**CCS Agents:**
- mmhs (Agent 3): Python WebSocket server (binary frames)
- mm (Agent 2): JS WebSocket client + Canvas rendering

#### 4.1 Python → JS binary frame protocol

```python
# Python server: send JPEG binary over WebSocket
async def swap_live(ws: WebSocket):
    await ws.accept()
    cap = cv2.VideoCapture(camera_index)
    while True:
        ret, frame = cap.read()
        if not ret: continue
        processed = process_frame(frame)  # existing pipeline
        _, jpeg = cv2.imencode('.jpg', processed, [cv2.IMWRITE_JPEG_QUALITY, 85])
        await ws.send_bytes(jpeg.tobytes())
```

```typescript
// JS client: receive binary, render to canvas
const ws = new WebSocket('ws://localhost:8008/swap/live');
ws.binaryType = 'arraybuffer';
ws.onmessage = (event) => {
  const blob = new Blob([event.data], { type: 'image/jpeg' });
  const url = URL.createObjectURL(blob);
  const img = new Image();
  img.onload = () => {
    ctx.drawImage(img, 0, 0);
    URL.revokeObjectURL(url);
  };
  img.src = url;
};
```

#### 4.2 Performance targets

- 30fps at 720p: ~33ms per frame budget
- JPEG encoding: ~5ms (quality 85)
- WebSocket TX: ~2ms (localhost)
- JS decode + Canvas draw: ~5ms
- **Available for inference:** ~21ms

#### 4.3 Adaptive frame rate

If backend can't keep up:
- Skip frames (don't buffer)
- Reduce JPEG quality dynamically
- Report actual FPS to StatusBar

---

### Week 5: Platform Installers

**CCS Agents:**
- mmhs (Agent 4): Build scripts per platform
- mm (Agent 5): Testing on each platform

#### 5.1 Build matrix

| Platform | Sidecar Build | Tauri Bundle | Installer |
|----------|--------------|-------------|-----------|
| Linux x86_64 | PyInstaller + PBS | `cargo tauri build` | AppImage, .deb |
| Windows x86_64 | PyInstaller + PBS | `cargo tauri build` | MSI (WiX), NSIS |
| macOS aarch64 | PyInstaller + PBS | `cargo tauri build` | DMG |

#### 5.2 CI pipeline (GitHub Actions)

```yaml
# .github/workflows/build.yml
jobs:
  build:
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
      - uses: dtolnay/rust-toolchain@stable
      - run: ./scripts/build-sidecar-${{ matrix.os }}.sh
      - run: npm ci && npm run tauri build
      - uses: actions/upload-artifact@v4
```

#### 5.3 GPU runtime handling

- **Default:** CPU inference (no CUDA required to install)
- **Optional:** First-run GPU detection, prompt to download CUDA runtime
- **Config:** Store GPU preference in `~/.config/deep-live-cam/config.json`

---

### Week 6: Auto-Update + System Tray + Polish

**CCS Agents:**
- mmhs (Agent 3): Auto-update + system tray
- mm (Agent 2): UI polish, error states, loading indicators
- mm (Agent 5): End-to-end testing

#### 6.1 Auto-update (tauri-plugin-updater)

```json
// tauri.conf.json
{
  "plugins": {
    "updater": {
      "endpoints": [
        "https://github.com/PhenixStar/Deep-Live-Cam/releases/latest/download/latest.json"
      ],
      "pubkey": "<ED25519_PUBLIC_KEY>"
    }
  }
}
```

Release workflow:
1. Tag release (`git tag v1.0.0`)
2. CI builds all platforms
3. Sign artifacts with Tauri key
4. Upload to GitHub Releases with `latest.json` manifest
5. Running apps auto-detect update on next launch

#### 6.2 System tray

- Minimize to tray (keep sidecar running)
- Tray menu: Show/Hide, Settings, Quit
- Status indicator: green (connected), red (sidecar crashed)

#### 6.3 Error handling

- Sidecar crash recovery: auto-restart Python process
- WebSocket disconnect: reconnect with exponential backoff
- Model download failure: retry with progress bar
- GPU not available: graceful fallback to CPU with notification

---

## Dependencies

| This Step | Depends On | Blocks |
|:-:|---|---|
| Week 1 (scaffold) | None | Week 2, 3 |
| Week 2 (bundling) | Week 1 | Week 4, 5 |
| Week 3 (UI) | Week 1 | Week 4 |
| Week 4 (streaming) | Weeks 2, 3 | Week 6 |
| Week 5 (installers) | Week 2 | Week 6 |
| Week 6 (polish) | Weeks 4, 5 | Ship |
| API contract | WS2 Week 1 (shared) | Week 4 server impl |

## Success Criteria

- [ ] Installable app on Linux (AppImage), Windows (MSI), macOS (DMG)
- [ ] 30fps live preview via WebSocket binary streaming
- [ ] Feature parity with CustomTkinter UI (image, video, live modes)
- [ ] Auto-update from GitHub Releases
- [ ] System tray with minimize/restore
- [ ] CPU fallback when no GPU detected
- [ ] Sidecar crash auto-recovery
- [ ] Bundle size < 300MB per platform (excluding CUDA)

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|:-:|:-:|---|
| PyInstaller + ONNX Runtime GPU bundling broken | High | High | Test early (Week 2); fallback: ship Python dir + launcher script |
| WebSocket frame rate < 30fps | Medium | Medium | Adaptive quality; skip frames; test on V100 early |
| macOS code signing / notarization | Medium | Medium | Defer macOS to v1.1 if signing cert not available |
| python-build-standalone missing wheels | Low | Medium | Pre-install wheels from PyPI; fallback to system Python |
| Tauri v2 breaking changes | Low | Low | Pin Tauri version; follow stable release |
| Frontend feature parity takes > 1 week | Medium | Low | Prioritize Live tab; defer Video batch to v1.1 |

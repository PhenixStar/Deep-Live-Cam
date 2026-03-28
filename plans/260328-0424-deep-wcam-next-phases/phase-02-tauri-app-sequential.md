# Phase 2: Tauri Desktop App (Approach B — Sequential)

**Workstream:** WS3 (runs as Phase 2 in sequential pipeline)
**Owner:** Team 3 Leader (primary) + Team 2 Leader (Rust/Tauri core)
**Timeline:** Weeks 2-4 (after WS1 completes)
**Agents:** 5 CCS-delegated (Team 3) + 3 reassigned from Team 1

---

## Objective

Ship a v1.0 Tauri desktop app with Python sidecar backend in 3 weeks. This is a compressed sprint — feature parity with current CustomTkinter UI is the goal, not perfection. Polish deferred to v1.1.

## Differences from Approach A (phase-03-tauri-app.md)

| Aspect | Approach A (6 weeks) | Approach B (3 weeks) |
|--------|---------------------|---------------------|
| Timeline | Weeks 1-6 | Weeks 2-4 |
| UI scope | Full feature parity | Essential features only |
| System tray | Week 6 | Deferred to v1.1 |
| Auto-update | Week 6 | Deferred to v1.1 |
| macOS DMG | Week 5 | Deferred (Linux + Windows first) |
| Polish | Comprehensive | Functional minimum |
| Team size | 5 agents | 8 agents (3 from Team 1) |

---

## Week 2: Scaffold + Sidecar + FastAPI (Days 1-5)

**CCS Agents:**
- Claude (Team 2, Agent 1): Tauri config, API contract design
- mmhs (Team 3, Agent 3): FastAPI server wrapping existing modules
- mmhs (Team 3, Agent 4): Build scripts (PyInstaller + python-build-standalone)
- mmhs (Team 1, Agent 1): Sidecar binary build per platform (reassigned)

### Day 1-2: Tauri + FastAPI skeleton

**Tauri init:**
```bash
npm create tauri-app@latest deep-live-cam-app -- --template react-ts --manager npm
```

**FastAPI server:** Wrap existing Deep-Live-Cam modules behind HTTP/WS endpoints.

Endpoints (API contract — shared with future Rust backend):
```
GET  /health                → { status, gpu, model, version }
POST /swap/image            → multipart(source, target) → JPEG
WS   /swap/live             → binary WebSocket (JPEG frames, bidirectional controls)
POST /config                → { provider, model, quality, mouth_mask, many_faces }
GET  /cameras               → [{ id, name }]
POST /cameras/select        → { camera_id }
```

**Delegation:** Claude writes API contract doc. mmhs implements FastAPI endpoints by importing from `modules/`.

### Day 3-4: Python sidecar bundling

1. Download python-build-standalone for Linux x86_64 and Windows x86_64
2. Install project deps + FastAPI/uvicorn into standalone Python
3. PyInstaller single-file build with ONNX Runtime hooks
4. Validate binary starts, loads model, responds to `/health`

**Critical test:** ONNX Runtime GPU provider loads correctly in PyInstaller bundle.

Fallback if PyInstaller fails:
```
# Ship Python directory instead of single binary
bin/
├── python/           # python-build-standalone
├── dlc-server.py     # Entry point
└── modules/          # Deep-Live-Cam modules
```

### Day 5: Integration test

- Tauri spawns sidecar → sidecar starts → `/health` responds
- Kill sidecar → Tauri detects death → restarts sidecar
- Test on Linux + Windows (macOS deferred)

---

## Week 3: Web UI + WebSocket Streaming (Days 6-10)

**CCS Agents:**
- mm (Team 3, Agent 2): React UI components
- mmhs (Team 3, Agent 3): WebSocket binary streaming (Python side)
- Claude (Team 2, Agent 1): WebSocket client integration
- mm (Team 1, Agent 3): Testing (reassigned)
- mm (Team 1, Agent 4): UI testing (reassigned)

### Day 6-7: Core UI (React + Tailwind)

**MVP tabs (must-have):**

1. **Live Tab** (highest priority)
   - Camera selector dropdown (populated from `/cameras`)
   - Video canvas (WebSocket binary frames)
   - Source face image upload
   - Start/Stop button
   - FPS counter

2. **Image Tab**
   - Source image upload
   - Target image upload
   - Process button → result viewer
   - Download result button

3. **Settings Tab**
   - Execution provider dropdown (CPU/CUDA)
   - Face enhancer toggle
   - Mouth mask toggle
   - JPEG quality slider

**Deferred to v1.1:**
- Video batch processing tab
- Multi-face mapping UI
- Theme toggle
- Tooltips

### Day 8-9: WebSocket binary frame streaming

**Python server (binary frames):**
```python
@app.websocket("/swap/live")
async def swap_live(ws: WebSocket):
    await ws.accept()
    cap = cv2.VideoCapture(camera_id)
    try:
        while True:
            ret, frame = cap.read()
            if not ret: continue
            processed = pipeline.process_frame(frame)
            _, jpeg = cv2.imencode('.jpg', processed, [cv2.IMWRITE_JPEG_QUALITY, 85])
            await ws.send_bytes(jpeg.tobytes())

            # Receive control messages
            try:
                msg = await asyncio.wait_for(ws.receive_json(), timeout=0.001)
                handle_control(msg)  # quality, mask toggle, etc.
            except asyncio.TimeoutError:
                pass
    finally:
        cap.release()
```

**JS client (Canvas rendering):**
```typescript
function LiveCanvas({ wsUrl }: { wsUrl: string }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const ws = new WebSocket(wsUrl);
    ws.binaryType = 'arraybuffer';

    ws.onmessage = (event) => {
      const blob = new Blob([event.data], { type: 'image/jpeg' });
      createImageBitmap(blob).then((bitmap) => {
        const ctx = canvasRef.current?.getContext('2d');
        ctx?.drawImage(bitmap, 0, 0);
      });
    };

    return () => ws.close();
  }, [wsUrl]);

  return <canvas ref={canvasRef} width={1280} height={720} />;
}
```

### Day 10: End-to-end test

- Launch app → sidecar starts → UI loads
- Select camera → live preview → face swap active
- Upload source/target → image swap → download result
- Change execution provider → verify GPU/CPU switch
- Test on Linux + Windows

---

## Week 4: Installers + QA (Days 11-15)

**CCS Agents:**
- mmhs (Team 3, Agent 4): Platform build scripts
- mm (Team 3, Agent 5): QA testing
- mm (Team 1, Agent 3): Cross-platform testing (reassigned)

### Day 11-12: Platform installers

**Linux:**
```bash
cargo tauri build --target x86_64-unknown-linux-gnu
# Outputs: AppImage + .deb
```

**Windows:**
```bash
cargo tauri build --target x86_64-pc-windows-msvc
# Outputs: MSI + NSIS installer
```

**Test matrix:**

| Platform | Installer | GPU | CPU | Live | Image |
|----------|-----------|:---:|:---:|:----:|:-----:|
| Ubuntu 22.04 | AppImage | Test | Test | Test | Test |
| Ubuntu 24.04 | .deb | Test | Test | Test | Test |
| Windows 10 | MSI | Test | Test | Test | Test |
| Windows 11 | NSIS | Test | Test | Test | Test |

### Day 13-14: Bug fixes + QA

- Fix installer issues found in testing
- Handle edge cases: no camera, no GPU, model download failure
- Error messages for common failures
- First-run experience: model download progress bar

### Day 15: v1.0 Release

```bash
# Tag and release
git tag v1.0.0
gh release create v1.0.0 \
  dist/deep-live-cam_1.0.0_amd64.AppImage \
  dist/deep-live-cam_1.0.0_amd64.deb \
  dist/deep-live-cam_1.0.0_x64.msi \
  --title "Deep-Live-Cam v1.0.0 - Desktop App" \
  --generate-notes
```

---

## Dependencies

| This Step | Depends On | Blocks |
|:-:|---|---|
| Week 2 | WS1 complete (stable Python) | Week 3 |
| Week 3 | Week 2 (sidecar works) | Week 4 |
| Week 4 | Week 3 (UI complete) | WS2 Rust migration |
| API contract (Week 2) | None | WS2 Rust binary (Weeks 5-8) |

## Success Criteria

- [ ] Installable on Linux (AppImage) and Windows (MSI)
- [ ] Live face swap at 30fps via WebSocket
- [ ] Image face swap with download
- [ ] CPU fallback when no GPU
- [ ] Sidecar auto-restart on crash
- [ ] Bundle size < 300MB per platform
- [ ] v1.0 tag on GitHub with release artifacts

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|:-:|:-:|---|
| 3-week sprint too tight | Medium | Medium | Cut Video tab and multi-face mapping to v1.1 |
| PyInstaller ONNX GPU fails | High | High | Ship Python directory bundle instead of single binary |
| WebSocket < 30fps | Medium | Medium | Reduce default to 720p; adaptive JPEG quality |
| Windows build issues | Medium | Medium | Team 1 agents assist; defer Windows to Day 14 if needed |
| No macOS build | Expected | Low | Acceptable for v1.0; macOS in v1.1 with code signing |

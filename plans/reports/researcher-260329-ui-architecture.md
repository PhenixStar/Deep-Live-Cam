# UI Architecture Research: Native-Feeling Tauri v2 + React for Real-Time Video

**Date:** 2026-03-29
**Researcher:** Research Agent
**Scope:** Five topic areas for expanding deep-forge-build beyond current MVP

---

## Codebase Baseline (What Already Exists)

Before recommendations, this documents what is already implemented so no work is duplicated.

| Area | Current State | Files |
|------|---------------|-------|
| Video stream | WebSocket → binary JPEG → `createImageBitmap` → canvas | `app/src/App.tsx` |
| FPS counter | Client-side frame count per second displayed in status bar | `App.tsx:75-80` |
| Camera selection | `GET /cameras` + `POST /camera/{index}` + dropdown in UI | `router.rs:399-425`, `App.tsx:95-115` |
| Enhancer toggles | `GET/POST /settings`, three checkboxes | `router.rs:427-458`, `App.tsx:117-127` |
| Model state | `AppState.models_dir`, `Models { detector, swapper }` — both `Mutex<Option<...>>` | `state.rs`, `router.rs:27-40` |
| Tauri shell | Spawns `dlc-server` sidecar, exposes `get_backend_url` command, auto-update plugin | `main.rs` |
| Backend | Axum on :8008, CORS for `tauri.localhost` + `tauri://localhost` | `router.rs:56-80` |

The frontend currently talks to the backend exclusively over HTTP/WebSocket at `http://localhost:8008`. There are no Tauri `invoke` calls for any business logic — only `@tauri-apps/plugin-updater` is used. This is a clean separation and should be maintained.

---

## Topic 1: System Metrics Display (CPU%, GPU%, VRAM, RAM)

### Architecture Decision: Tauri Commands vs Polling Endpoint

Two viable patterns exist:

**Pattern A — Tauri `invoke` command (recommended)**
```
React → tauri.invoke("get_metrics") → main.rs command → sysinfo → JSON response
```
- Zero HTTP overhead, no CORS concern, no extra socket
- Metrics stay in the Tauri shell process (appropriate: shell already owns the process lifecycle)
- Tauri's IPC is synchronous from the Rust side; no async overhead for a 50ms poll

**Pattern B — HTTP endpoint on dlc-server**
- Would require adding `sysinfo` dep to the server crate
- Server is stateless inference infrastructure; system metrics don't belong there
- Adds unnecessary coupling

Use Pattern A.

### Rust Implementation (`app/src-tauri/src/main.rs`)

Add `sysinfo` to `app/src-tauri/Cargo.toml`:
```toml
sysinfo = "0.33"
```

Version 0.33 is the current stable as of early 2026. The `sysinfo` crate uses a `System` struct that must be refreshed explicitly — it does not auto-poll.

```rust
use sysinfo::{System, SystemExt, CpuExt, ProcessExt};

#[derive(serde::Serialize)]
struct Metrics {
    cpu_percent: f32,
    ram_used_mb: u64,
    ram_total_mb: u64,
    // GPU fields populated only if nvml is available
    gpu_percent: Option<f32>,
    vram_used_mb: Option<u64>,
    vram_total_mb: Option<u64>,
}

// State: System is expensive to construct; store in managed state
struct MetricsState(Mutex<System>);

#[tauri::command]
fn get_metrics(state: tauri::State<MetricsState>) -> Metrics {
    let mut sys = state.0.lock().unwrap();
    sys.refresh_cpu();
    sys.refresh_memory();
    let cpu = sys.global_cpu_info().cpu_usage();
    let ram_used = sys.used_memory() / 1024 / 1024;
    let ram_total = sys.total_memory() / 1024 / 1024;
    // GPU via nvml-wrapper (optional feature)
    Metrics { cpu_percent: cpu, ram_used_mb: ram_used, ram_total_mb: ram_total,
              gpu_percent: None, vram_used_mb: None, vram_total_mb: None }
}
```

Register in `setup()`:
```rust
app.manage(MetricsState(Mutex::new(System::new_all())));
```

Add to `invoke_handler`:
```rust
.invoke_handler(tauri::generate_handler![get_backend_url, get_metrics])
```

### GPU/VRAM on Windows

`sysinfo` does not expose GPU VRAM. Use `nvml-wrapper` (wraps NVIDIA NVML) as a conditional feature:

```toml
[dependencies]
nvml-wrapper = { version = "0.10", optional = true }

[features]
nvidia = ["nvml-wrapper"]
```

On AMD, `wgpu` can query adapter info but does not expose used VRAM. The practical approach for v1: show GPU% and VRAM only on NVIDIA; show `null` otherwise and let the UI hide those fields gracefully.

### React Polling Pattern

Poll interval: **2000ms** for metrics. Video frames arrive at 30fps; metrics at 0.5fps is more than sufficient. Do not poll faster — `sysinfo` `refresh_cpu()` does real syscalls.

```typescript
// hooks/useSystemMetrics.ts
import { invoke } from "@tauri-apps/api/core";

interface SystemMetrics {
  cpu_percent: number;
  ram_used_mb: number;
  ram_total_mb: number;
  gpu_percent: number | null;
  vram_used_mb: number | null;
  vram_total_mb: number | null;
}

export function useSystemMetrics(intervalMs = 2000) {
  const [metrics, setMetrics] = useState<SystemMetrics | null>(null);
  useEffect(() => {
    const poll = () => invoke<SystemMetrics>("get_metrics").then(setMetrics).catch(() => {});
    poll();
    const id = setInterval(poll, intervalMs);
    return () => clearInterval(id);
  }, [intervalMs]);
  return metrics;
}
```

Display as a compact status strip at the bottom of the window, not in the controls sidebar. Keep it to single-line text: `CPU 12% | RAM 4.2/16 GB | GPU 34% | VRAM 2.1/8 GB`. Hide GPU/VRAM cells when values are `null`.

### Polling Interval Guidelines

| Metric | Refresh Rate | Rationale |
|--------|-------------|-----------|
| CPU% | 2s | Kernel averages over ~1s; polling faster adds no signal |
| RAM | 5s | Rarely changes fast enough to matter |
| GPU% / VRAM | 2s | NVML queries are cheap but GPU% is already smoothed by driver |
| FPS counter | Already done client-side per-second in `App.tsx` | No change needed |

---

## Topic 2: Model Management UI Patterns

### Current State

`AppState.models_dir` holds the path. `Models { detector: Mutex<Option<FaceDetector>>, swapper: Mutex<Option<FaceSwapper>> }` — `None` means not loaded. When `None`, handlers return HTTP 503 with `"models not loaded"` error. The UI currently shows no model status.

### What to Expose from the Backend

Add a `GET /models/status` endpoint to `router.rs`:

```rust
#[derive(serde::Serialize)]
struct ModelStatus {
    detector: ModelFileStatus,
    swapper: ModelFileStatus,
    enhancers: Vec<EnhancerStatus>,
}

#[derive(serde::Serialize)]
struct ModelFileStatus {
    loaded: bool,
    file_exists: bool,
    file_size_mb: Option<f64>,
    path: String,
}
```

This lets the UI distinguish three states: loaded in memory, file present but not loaded, and file missing (needs download).

### Download Progress Pattern

The reference pattern used by AI desktop apps (LM Studio, Ollama Desktop, Jan.ai) is:

1. Show model name, expected size, and a "Download" button when file is absent
2. On click, start download and stream progress via SSE or WebSocket
3. Display a progress bar with bytes downloaded / total bytes and estimated time remaining
4. Auto-load the model after download completes
5. Show a persistent "loaded" badge when the model is in memory

For this codebase, the cleanest approach is a **Tauri command** for downloads (not an HTTP endpoint), because:
- Tauri `emit` lets Rust push progress events to React without polling
- File system access is native to Tauri; no need to proxy through the local HTTP server
- The download should survive if the HTTP server restarts

```rust
// In main.rs
#[tauri::command]
async fn download_model(
    app: tauri::AppHandle,
    url: String,
    dest_filename: String,
) -> Result<(), String> {
    // Use reqwest to stream the download
    // Emit progress events: app.emit("model_download_progress", payload)
    // Payload: { filename, downloaded_bytes, total_bytes, percent }
}
```

React listens with `@tauri-apps/api/event`:
```typescript
import { listen } from "@tauri-apps/api/event";

useEffect(() => {
  const unlisten = listen<DownloadProgress>("model_download_progress", (event) => {
    setDownloadProgress(event.payload);
  });
  return () => { unlisten.then(fn => fn()); };
}, []);
```

### Model File Detection on Startup

Add startup logic in `main.rs` `setup()` to check for model files and emit a `models_status` event after 1 second (giving the webview time to load). This avoids a React polling race on startup:

```rust
// After sidecar spawn, check model files and emit status
app.emit("models_status", check_model_files(&models_dir)).ok();
```

### UI Component Structure

```
ModelPanel (collapsible sidebar section)
  ModelRow — "Face Detector (SCRFD)"
    [loaded badge] OR [1.2 MB file found] OR [Download 1.2 MB ▼]
    Progress bar (visible during download only)
  ModelRow — "Face Swapper (InSwapper-128)"
    [loaded badge] OR [Download 512 MB ▼]
  ModelRow — "Face Enhancer (GFPGAN)"
    [loaded badge] OR [Download 348 MB ▼]
```

Keep file size display accurate: pull `content-length` header during download, fall back to hardcoded known sizes as a UX hint before download starts.

### Auto-Detection of Missing Models

The server already returns 503 with `"models not loaded"` when inference is attempted without models. The UI should intercept this at the WebSocket connect point:

```typescript
// Before connecting WebSocket, check /health
// If models are missing, show ModelPanel prominently instead of attempting connect
const health = await fetch(`${API_BASE}/health`);
// Add models_loaded field to health response
```

This prevents the confusing "connection failed" error when the real issue is missing model files.

---

## Topic 3: Input/Output Source Selection

### Camera Enumeration (Already Implemented, Extension Points)

The current `dlc-capture` crate probes indices 0-9 on Linux and uses OpenCV on Windows/macOS. Improvements to consider:

**Better naming on Windows**: Use `Windows.Media.Capture.Frames` API via the `windows` crate to get friendly device names (e.g., "Logitech C920" vs "Camera 0"). The current stub returns generic names.

**Virtual camera detection**: OBS Virtual Camera appears as a standard DirectShow device on Windows. It will already appear in the camera list — no special handling needed. Label filtering (if name contains "OBS" or "Virtual") can add a distinguishing badge in the UI.

**USB hot-plug**: Cameras plugged in after app start won't appear. Add a "Refresh" button next to the dropdown. For automatic refresh, Tauri's `tauri-plugin-fs` watch API can monitor device changes, but this is complex on Windows (requires WMI queries). The "Refresh" button is sufficient for v1.

### Video File Input

Allow a video file (MP4, AVI, MKV) as the source instead of a live camera. Architecture:

- Add a `source_type` field to `AppState`: `enum SourceType { Camera(u32), File(PathBuf), Rtsp(String) }`
- `dlc-capture` `CameraCapture::open_file(path)` variant — OpenCV `VideoCapture` accepts file paths with the same API as camera indices
- React: file picker via `@tauri-apps/plugin-dialog` (not `<input type="file">` — Tauri dialog gives native OS file picker)
- The video file plays frame-by-frame at its native FPS; the WebSocket ticker interval should be adjusted to match source FPS

### RTSP Stream Input

OpenCV `VideoCapture` accepts RTSP URLs directly:
```rust
VideoCapture::from_str("rtsp://192.168.1.100:554/stream", CAP_ANY)
```

The UI needs a text input for the RTSP URL. No special backend changes beyond the `source_type` enum. Latency will be higher (network buffering); warn the user with an info tooltip.

### Virtual Camera Output (Writing processed frames out)

This is significantly more complex than input. Options ranked by feasibility for this codebase:

**Option A — OBS Plugin approach (not applicable here)**
OBS Virtual Camera is an output of OBS, not something an external app writes to.

**Option B — v4l2loopback (Linux only)**
Create a virtual `/dev/videoN` device that other apps (OBS, browser) can read. Requires `sudo modprobe v4l2loopback`. OpenCV can write to it:
```rust
VideoWriter::new("/dev/video2", VideoWriter::fourcc('Y','U','Y','V',), fps, size, true)
```
This is the most practical virtual camera output on Linux.

**Option C — AkVirtualCamera (Windows/macOS)**
The `akvcam` driver creates a virtual DirectShow device on Windows. It requires driver installation (user must approve admin prompt). Used by Snap Camera, Deep Live Cam Python version.

**Option D — Named pipe / shared memory (simplest for developer tooling)**
Write frames to a named pipe; other tools (FFmpeg, OBS "Video Capture Device" with pipe source) can consume them. No driver required. Not visible as a webcam to video-call apps.

**Recommendation for v1**: Implement video file output (save processed stream to MP4) using OpenCV `VideoWriter`. This is straightforward and covers the "I want to save a face-swap video" use case. Virtual camera output (Option B/C) is a v2 feature due to driver complexity.

For file output:
```rust
// Add to AppState
pub output_writer: Option<opencv::videoio::VideoWriter>,
pub recording: bool,
```

Add `POST /recording/start` and `POST /recording/stop` endpoints. The WebSocket `produce_frame` function writes frames to the writer when recording is active.

---

## Topic 4: Server/Remote Mode Patterns

### What Deep Live Cam Python Does

The Python version (`run.py`) has a `--server-host` / `--server-port` flag that changes FastAPI's bind address from `127.0.0.1` to `0.0.0.0`. That's the entire implementation — bind on all interfaces, document the port in the README, and let users configure their own firewall/network. No relay, no TURN/STUN, no auth.

The Rust server currently binds on `127.0.0.1:8008` (loopback only). Remote access requires changing the bind address.

### Patterns for Remote Clients

**Pattern A — Direct bind on LAN IP (simplest)**

Add a `--remote` flag to `dlc-server` that binds on `0.0.0.0:8008` instead of `127.0.0.1`. Update the CORS layer to allow any origin when in remote mode. The React frontend URL becomes configurable (not hardcoded `localhost:8008`).

This covers: same-LAN access from a tablet, phone, or secondary PC. No tunnel software needed.

Tauri UI addition: a "Remote Mode" toggle in settings that:
1. Calls a Tauri command to restart the sidecar with `--remote` flag
2. Displays the machine's LAN IP so the user can share it
3. Shows a warning: "Anyone on your network can control the face swap"

**Pattern B — Tailscale (recommended for internet access)**

Tailscale creates a private WireGuard mesh. If the user installs Tailscale, the Rust server on `0.0.0.0` is reachable at the Tailscale IP from anywhere in the world, with zero firewall configuration. This is what the deep-face-live community recommends for remote setups.

The app doesn't need to integrate Tailscale directly — just document it. Detection: check if `tailscale.exe` is running (Windows) and display the Tailscale IP in the Remote Mode panel.

**Pattern C — WebSocket relay / TURN/STUN**

Used for WebRTC-based tools where peers are behind strict NAT. Not applicable here — this is a client-server architecture, not peer-to-peer. TURN/STUN add unnecessary complexity. Avoid.

**Pattern D — SSH tunnel**

For developer/power users: `ssh -L 8008:localhost:8008 user@server`. Document in README; no app changes needed.

### Tauri `get_backend_url` Command

Currently hardcodes `http://localhost:8008`. For remote mode, this command should return the actual bind address. Extend it to read from a config file or environment variable that the sidecar writes on startup.

### Security for Remote Mode

When binding on `0.0.0.0`, add:
- A simple API token header check (`X-Deep-Forge-Token: <random_uuid>`)
- The token is generated on first run and stored in `AppState`
- The token is displayed in the Tauri UI so the user can share it with remote clients
- The Tauri frontend always sends the token automatically

This is not enterprise security, but it prevents accidental access from other LAN devices.

---

## Topic 5: Debug/Diagnostic Overlay Patterns

### Current State

- FPS counter: exists, shown in header as text (`status === "processing" && \`(${fps} fps)\``)
- No bounding boxes, no confidence scores, no latency, no memory graphs

### Architecture: Overlay Data via WebSocket Side-Channel

The cleanest approach is to extend the existing WebSocket connection to carry metadata alongside the frame. Two options:

**Option A — Multiplex metadata in binary message**

Add a small binary header before the JPEG bytes:
```
[4 bytes: metadata_len][metadata_len bytes: JSON][rest: JPEG]
```

The React client reads the header, parses JSON metadata, draws overlays on a separate `<canvas>` layered over the video canvas. This is zero additional connections but requires both sides to implement the framing protocol.

**Option B — Separate JSON WebSocket for metrics (recommended)**

Add a second WebSocket endpoint `GET /ws/metrics` that pushes a JSON message on each frame cycle:
```json
{
  "inference_ms": 45,
  "detect_ms": 12,
  "swap_ms": 28,
  "enhance_ms": 5,
  "face_count": 1,
  "faces": [
    { "x": 120, "y": 80, "w": 180, "h": 200, "confidence": 0.94 }
  ]
}
```

This keeps the video stream binary-only (good for JPEG decode performance) and the metrics stream independent (can be connected/disconnected separately). The overlay canvas subscribes to the metrics WebSocket.

### Bounding Box Overlay Implementation

Use a transparent `<canvas>` positioned absolutely over the video canvas via CSS:
```css
.canvas-container { position: relative; }
.video-canvas { position: absolute; top: 0; left: 0; }
.overlay-canvas { position: absolute; top: 0; left: 0; pointer-events: none; }
```

React draws on the overlay canvas when new face data arrives:
```typescript
function drawOverlays(ctx: CanvasRenderingContext2D, faces: FaceDetection[]) {
  ctx.clearRect(0, 0, ctx.canvas.width, ctx.canvas.height);
  ctx.strokeStyle = "#00ff00";
  ctx.lineWidth = 2;
  ctx.font = "12px monospace";
  ctx.fillStyle = "#00ff00";
  for (const face of faces) {
    ctx.strokeRect(face.x, face.y, face.w, face.h);
    ctx.fillText(`${(face.confidence * 100).toFixed(0)}%`, face.x, face.y - 4);
  }
}
```

Scale the bounding box coordinates: the detection runs on the original frame size, but the canvas may be displayed at a different CSS size. Use `canvas.width / canvas.getBoundingClientRect().width` as the scale factor.

### Inference Latency Tracking in Rust

Add timing to `produce_frame` in `router.rs`:
```rust
fn produce_frame(...) -> Option<(Vec<u8>, FrameMetadata)> {
    let t0 = std::time::Instant::now();
    // ... detect ...
    let detect_ms = t0.elapsed().as_millis() as u32;
    let t1 = std::time::Instant::now();
    // ... swap ...
    let swap_ms = t1.elapsed().as_millis() as u32;
    // ...
}
```

Store the last `FrameMetadata` in a `Arc<RwLock<FrameMetadata>>` in `ServerState`. The `/ws/metrics` handler reads it and pushes to connected clients.

### FPS Counter Enhancement

The current client-side FPS counter only measures WebSocket message receipt rate. For diagnostic purposes, also expose server-side FPS (frames actually produced by the pipeline, before network). Add `frames_produced: u64` and `last_frame_ts_ms: u64` to `FrameMetadata`. The metrics WebSocket computes server FPS from the delta.

### Memory Usage Graph

A sparkline graph showing the last 60 seconds of RAM usage provides intuitive drift detection (memory leak visibility). Implementation:

```typescript
// Keep a circular buffer of 60 samples
const [ramHistory, setRamHistory] = useState<number[]>([]);

useEffect(() => {
  // On each metrics poll, append to history
  setRamHistory(prev => [...prev.slice(-59), metrics.ram_used_mb]);
}, [metrics]);
```

Render with a simple `<canvas>` sparkline — no charting library needed for a 60-point line graph. Width: 120px, height: 30px.

### Debug Overlay Toggle

Expose via a keyboard shortcut (e.g., `D` key) and a button in the UI. Store the visibility state in `localStorage` so it persists across sessions. The overlay canvas is simply `display: none` when disabled — no WebSocket disconnection needed; let the metrics stream idle.

---

## Implementation Priority & Effort Estimates

| Feature | Complexity | Effort | Dependencies |
|---------|-----------|--------|--------------|
| System metrics display (CPU/RAM via sysinfo) | Low | 3h | Add `sysinfo` to Tauri Cargo.toml |
| Model status panel (file detection + 503 UX fix) | Low | 2h | None — router changes only |
| Model download with progress (Tauri command + emit) | Medium | 4h | `reqwest` in src-tauri |
| Inference latency overlay (Option B separate WS) | Medium | 5h | Struct changes in router.rs + new WS endpoint |
| Bounding box overlay canvas | Low | 2h | Requires latency overlay above |
| FPS counter server-side | Low | 1h | Part of latency overlay work |
| Remote mode (bind flag + LAN IP display) | Low | 3h | None |
| API token for remote mode | Low | 2h | After remote mode |
| Video file input | Medium | 4h | `source_type` enum in AppState + dlc-capture changes |
| Virtual camera output (Linux v4l2loopback) | High | 8h | v4l2loopback driver, platform-specific |
| GPU/VRAM metrics (NVIDIA only) | Medium | 3h | `nvml-wrapper` dep, conditional feature flag |
| Memory graph sparkline | Low | 1h | After system metrics |

---

## Key Architectural Decisions for This Codebase

### 1. Keep HTTP/WS for inference; use Tauri commands for shell concerns

The current split is correct and should be preserved:
- `dlc-server` (Axum): video, cameras, settings, models — stateless inference
- `src-tauri/main.rs` (Tauri commands): system metrics, file downloads, app config, sidecar lifecycle

Avoid adding system-level concerns (disk I/O, OS metrics, process management) to `dlc-server`. It should remain a pure inference server that could theoretically run headlessly.

### 2. Overlay canvas as a separate DOM element, not drawn on the video canvas

The video canvas is updated at 30fps via `createImageBitmap`. Drawing overlays in the same paint cycle requires synchronizing with the WebSocket `onmessage` handler. A separate overlay canvas updated by the metrics WebSocket is simpler and avoids frame-tearing on the video.

### 3. Model downloads belong in Tauri shell, not dlc-server

`dlc-server` should not manage its own model files. The Tauri shell knows the `resource_dir` and the target `models_dir`. Model download as a Tauri command keeps file management in one place and allows showing OS-level progress notifications.

### 4. Remote mode is a flag on dlc-server, not a proxy

Do not build an in-process WebSocket relay. The sidecar architecture already supports this: restart `dlc-server` with `--remote` flag and `0.0.0.0` bind. Keep it simple.

### 5. `AppState` in `dlc-server` needs a `source_type` field

Currently `active_camera: u32` is the only input source. Before implementing file input or RTSP, change this to an enum. This is a small refactor that unblocks multiple features.

---

## Relevant File Paths

| File | Relevant To |
|------|-------------|
| `D:/Dev/deep-forge-build/app/src/App.tsx` | All frontend topics |
| `D:/Dev/deep-forge-build/app/src-tauri/src/main.rs` | System metrics, model download, remote mode commands |
| `D:/Dev/deep-forge-build/core/rust-engine/dlc-server/src/router.rs` | Inference latency, bounding boxes, model status endpoint |
| `D:/Dev/deep-forge-build/core/rust-engine/dlc-server/src/state.rs` | `source_type` enum, recording state, metrics state |
| `D:/Dev/deep-forge-build/core/rust-engine/dlc-capture/src/lib.rs` | Video file input, RTSP input, virtual camera output |
| `D:/Dev/deep-forge-build/core/rust-engine/Cargo.toml` | Workspace deps for new crates |

---

## Crate References

| Crate | Version | Purpose |
|-------|---------|---------|
| `sysinfo` | 0.33 | CPU%, RAM, process info — pure Rust, cross-platform |
| `nvml-wrapper` | 0.10 | NVIDIA GPU%/VRAM — Windows + Linux, NVIDIA-only |
| `reqwest` | 0.12 | Async HTTP for model downloads in Tauri shell |
| `v4l2` | 0.14 | Linux virtual camera output (v4l2loopback write) |
| `tauri-plugin-dialog` | 2.x | Native OS file picker for video file input |
| `tauri-plugin-notification` | 2.x | OS notification when model download completes |

---

*Report generated by research agent. All code snippets are illustrative; exact API signatures should be verified against crate documentation before implementation.*

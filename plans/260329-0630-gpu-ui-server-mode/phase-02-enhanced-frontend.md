# Phase 2: Enhanced Frontend — Debug Overlay, Metrics, Source Scoring

**Priority:** P0
**Effort:** 10h
**Status:** Done (~95%) — audited 2026-03-29
**Depends on:** Phase 1 (GPU metrics)

## Overview

Add real-time debug information, face detection scoring, system metrics, and a native-feeling control panel to the Tauri frontend.

## Key Insights

- System metrics (CPU/RAM/GPU) belong in Tauri shell via `sysinfo` crate, not dlc-server
- Face detection metadata (bounding boxes, scores, inference timing) comes from `/ws/metrics` JSON endpoint
- Overlay renders on a transparent canvas stacked above the video canvas
- Poll system metrics at 2s intervals; inference metrics arrive per-frame via WS

## Architecture

```
┌─────────────────────────────────────┐
│          React Frontend             │
│                                     │
│  ┌──────────┐  ┌─────────────────┐  │
│  │ Video    │  │ Metrics Panel   │  │
│  │ Canvas   │  │ CPU: 23%        │  │
│  │ (binary) │  │ RAM: 4.2 GB     │  │
│  │          │  │ GPU: 67%        │  │
│  │ ┌──────┐ │  │ VRAM: 1.8 GB    │  │
│  │ │Debug │ │  │ FPS: 14         │  │
│  │ │Over- │ │  │ Detect: 8ms     │  │
│  │ │lay   │ │  │ Swap: 52ms      │  │
│  │ └──────┘ │  │ Score: 0.87     │  │
│  └──────────┘  └─────────────────┘  │
│                                     │
│  ┌─────────────────────────────────┐│
│  │ Controls: Source | Camera | EP  ││
│  └─────────────────────────────────┘│
└─────────────────────────────────────┘
         │                    │
    WS /ws/video         WS /ws/metrics
    (binary JPEG)        (JSON per frame)
         │                    │
┌─────────────────────────────────────┐
│       dlc-server (Rust)             │
└─────────────────────────────────────┘
```

## Related Code Files

### Modify
- `app/src/App.tsx` — Split into components, add metrics panel + debug overlay
- `app/src/styles.css` — Layout for metrics panel and overlay
- `core/rust-engine/dlc-server/src/router.rs` — Add `/ws/metrics` endpoint, timing in `produce_frame`

### Create
- `app/src/components/video-canvas.tsx` — Video + overlay canvas component
- `app/src/components/metrics-panel.tsx` — CPU/RAM/GPU/FPS/inference timing
- `app/src/components/controls-panel.tsx` — Source, camera, enhancers, EP selector
- `app/src/components/debug-overlay.tsx` — Bounding boxes + scores on canvas
- `app/src/hooks/use-metrics-ws.ts` — WebSocket hook for `/ws/metrics`
- `app/src/hooks/use-system-metrics.ts` — Tauri invoke hook for system stats
- `app/src-tauri/src/metrics.rs` — Tauri commands for CPU/RAM/GPU via sysinfo

## Implementation Steps

### Step 1: Backend — Add inference timing to produce_frame

In `router.rs`, wrap detection and swap calls with `std::time::Instant`:
```rust
struct FrameMetrics {
    detect_ms: f64,
    swap_ms: f64,
    total_ms: f64,
    face_count: usize,
    faces: Vec<FaceInfo>,  // bbox + score per face
}
struct FaceInfo {
    x: f32, y: f32, w: f32, h: f32,
    score: f32,
}
```

Return `(Option<Vec<u8>>, FrameMetrics)` from `produce_frame`.

### Step 2: Backend — Add /ws/metrics endpoint

New WS endpoint that pushes JSON per frame:
```json
{
  "detect_ms": 8.2,
  "swap_ms": 52.1,
  "total_ms": 65.4,
  "face_count": 1,
  "faces": [{"x": 120, "y": 80, "w": 200, "h": 240, "score": 0.87}],
  "ep": "DirectML",
  "frame_id": 1234
}
```

Broadcast from the video WS handler via a `tokio::sync::broadcast` channel.

### Step 3: Backend — Add /health extended info

Extend `/health` response:
```json
{
  "status": "ok",
  "backend": "rust",
  "ep": "DirectMLExecutionProvider",
  "models": {
    "detector": true,
    "swapper": true,
    "gfpgan": false
  }
}
```

### Step 4: Tauri — System metrics commands

Add `sysinfo = "0.33"` to `app/src-tauri/Cargo.toml`. Create `metrics.rs`:
```rust
#[tauri::command]
fn get_system_metrics(state: State<MetricsState>) -> SystemMetrics {
    let mut sys = state.0.lock().unwrap();
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    SystemMetrics {
        cpu_percent: sys.global_cpu_usage(),
        ram_used_gb: sys.used_memory() as f64 / 1_073_741_824.0,
        ram_total_gb: sys.total_memory() as f64 / 1_073_741_824.0,
    }
}
```

### Step 5: Frontend — Split App.tsx into components

Extract from monolithic App.tsx:
- `VideoCanvas` — video + debug overlay (two stacked canvases)
- `MetricsPanel` — system metrics (polled via Tauri invoke) + inference metrics (via /ws/metrics)
- `ControlsPanel` — source upload, camera select, enhancers, EP selector
- `App` — layout shell connecting components

### Step 6: Frontend — Debug overlay

Transparent canvas positioned over video canvas:
```tsx
// In debug-overlay.tsx
function drawBoundingBoxes(ctx, faces, canvasW, canvasH, frameW, frameH) {
  ctx.clearRect(0, 0, canvasW, canvasH);
  const scaleX = canvasW / frameW;
  const scaleY = canvasH / frameH;
  for (const face of faces) {
    ctx.strokeStyle = `rgba(0, 255, 0, ${face.score})`;
    ctx.lineWidth = 2;
    ctx.strokeRect(face.x * scaleX, face.y * scaleY, face.w * scaleX, face.h * scaleY);
    ctx.fillStyle = '#0f0';
    ctx.font = '12px monospace';
    ctx.fillText(`${(face.score * 100).toFixed(0)}%`, face.x * scaleX, face.y * scaleY - 4);
  }
}
```

### Step 7: Frontend — Metrics panel

Displays in a sidebar:
- CPU% (bar)
- RAM used/total
- GPU% (if available)
- VRAM used/total (if available)
- FPS (from existing counter)
- Detection latency (from /ws/metrics)
- Swap latency (from /ws/metrics)
- Source face score (best detection confidence)

## Todo

- [x] Add timing instrumentation to `produce_frame`
- [x] Create `FrameMetrics` struct and broadcast channel
- [x] Add `/ws/metrics` WS endpoint
- [x] Extend `/health` with EP + model status
- [x] Add `sysinfo` to Tauri shell, create metrics commands
- [x] Split `App.tsx` into components
- [x] Create `VideoCanvas` with debug overlay
- [x] Create `MetricsPanel` component
- [x] Create `ControlsPanel` (EP selector deferred — display-only)
- [x] Create `use-metrics-ws` hook
- [x] Create `use-system-metrics` hook
- [x] Style metrics panel and overlay
- [ ] GPU utilization metric (requires platform-specific lib, not available via sysinfo)

## Success Criteria

- Face bounding boxes + scores visible on debug overlay (toggle-able)
- System metrics (CPU/RAM) displayed and updating every 2s
- Inference timing (detect_ms, swap_ms) displayed per frame
- Source face shows detection score after upload
- EP name displayed in UI

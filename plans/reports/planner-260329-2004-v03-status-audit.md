# v0.3 Plan Audit — Actual Implementation Status

**Date:** 2026-03-29
**Auditor:** Team 2 Lead (planner)
**Plan:** `plans/260329-0630-gpu-ui-server-mode/plan.md`

---

## Phase Summary

| Phase | Plan Status | Actual Status | Completion |
|-------|-------------|---------------|------------|
| 1 — DirectML GPU | Pending | **DONE** | ~90% |
| 2 — Enhanced Frontend | Pending | **DONE** | ~95% |
| 3 — Model Management | Pending | **DONE** | ~95% |
| 4 — I/O Sources | Pending | **PARTIAL** | ~30% |
| 5 — Server Mode | Pending | **PARTIAL** | ~60% |

---

## Phase 1: DirectML GPU Acceleration — DONE (~90%)

### Completed
- `GpuProvider` enum in `dlc-core/src/lib.rs` with `Auto`, `DirectML { device_id }`, `Cpu`
- `GpuProvider::load_session()` centralizes EP configuration (memory_pattern=false for DirectML)
- `FaceDetector::new()` accepts `&GpuProvider`, uses `provider.load_session()`
- `FaceSwapper::new()` accepts `&GpuProvider`, uses `provider.load_session()` for both arcface + swap
- `FaceEnhancer::new()` accepts `&GpuProvider`, with CPU fallback on failure
- `main.rs` passes `GpuProvider::Auto` to all model constructors
- `scripts/setup-directml-dlls.ps1` created — downloads from NuGet v1.24.4
- `scripts/build-rust-sidecar-win.ps1` — sets `ORT_LIB_PATH`, copies DLLs to binaries dir
- `.cargo/config.toml` — documents `ORT_LIB_PATH` env var for Windows
- `/health` endpoint returns `gpu_provider` field

### Remaining
1. **ort Cargo feature not switched on Windows** — workspace Cargo.toml still uses `download-binaries` globally. Plan called for `features = ["directml", "copy-dylibs", "half"]`. Current: `features = ["std", "download-binaries", "tls-native"]`. On Windows with `ORT_LIB_PATH` set this works because ORT finds the DLLs, but `directml` feature flag is not explicitly enabled. Verify this doesn't affect EP registration.
2. **No `half` feature** — Plan mentioned `half` for FP16 support. Not present in workspace deps.
3. **EP validation in `/health`** — Current `/health` shows `gpu_provider: "Auto"` (a debug string of the config enum). Plan wanted to show the actually-selected EP name from the runtime (e.g., `DirectMLExecutionProvider`). The current value is the *requested* provider, not the *active* one.
4. **FPS benchmarking not documented** — No recorded baseline vs DirectML measurement in codebase.

### Assessment
Core work is done. The DirectML path is functional on Windows. The remaining items are polish and verification.

---

## Phase 2: Enhanced Frontend — DONE (~95%)

### Completed
- **FrameMetrics struct** in `router.rs` with `detect_ms`, `swap_ms`, `total_ms`, `face_count`, `faces: Vec<FaceRect>`, `swap_bbox`
- **`/ws/metrics` endpoint** — broadcast channel (`tokio::sync::broadcast`), JSON per frame
- **`produce_frame()` instrumented** — `Instant` timing around detect + swap, total_ms set before return
- **`/health` extended** — returns `backend`, `gpu_provider`, model status (detector/swapper/gfpgan/gpen256/gpen512), `remote_mode`, `bind_address`
- **Tauri `get_system_metrics` command** — `sysinfo` crate, returns `cpu_percent`, `ram_used_gb`, `ram_total_gb`
- **`app/src/components/video-canvas.tsx`** — dual canvas (video + overlay), bounding box rendering (green detected, red swap region)
- **`app/src/components/metrics-panel.tsx`** — inference section (FPS/detect/swap/total/faces/best score) + system section (CPU/RAM/GPU) + source score
- **`app/src/components/controls-panel.tsx`** — source upload, camera select+refresh, resolution selector, enhancer toggles, debug overlay toggle, swap calibration sliders, server mode display
- **`app/src/hooks/use-metrics-ws.ts`** — WS hook for `/ws/metrics`
- **`app/src/hooks/use-system-metrics.ts`** — polls Tauri `get_system_metrics` at 2s intervals
- **`App.tsx` refactored** — clean component composition (ControlsPanel + VideoCanvas + MetricsPanel + ModelManager)
- **Types extracted** to `app/src/types.ts`

### Remaining
1. **No GPU utilization metric** — `sysinfo` crate does not expose GPU%. Plan showed "GPU: 67%" and "VRAM: 1.8 GB" in metrics panel. Current MetricsPanel shows `gpuProvider` string, not actual GPU utilization. This requires a platform-specific solution (NVML for NVIDIA, `amdgpu_top` for AMD, or WMI on Windows).
2. **EP selector in controls** — Plan mentioned EP selector in ControlsPanel. Not implemented (would require runtime EP switching, which is expensive — session rebuild). Current: display-only. This is likely correctly deferred.

### Assessment
This phase is effectively complete. The GPU utilization gap is a nice-to-have that requires platform-specific work.

---

## Phase 3: Model Management — DONE (~95%)

### Completed
- **`GET /models/status` endpoint** — returns per-model name, file path, file_exists, size_mb, required flag
- **Tauri `download_model` command** — streams from URL, emits `model_download_progress` events with name/downloaded/total
- **Tauri `get_models_dir` command** — resolves resource_dir/models path
- **`app/src/components/model-manager.tsx`** — modal overlay with cards per model, status labels, download buttons, progress bars, missing-model warnings
- **`app/src/hooks/use-models.ts`** — fetches `/models/status`, listens to download progress events, `MODEL_URLS` with HuggingFace download URLs for all 6 models
- **Startup model check** — `App.tsx` shows banner + badge when required models are missing
- **Manual note** for models without download URLs (though all 6 have URLs currently)

### Remaining
1. **`loaded` field missing from `/models/status`** — Plan showed `"loaded": true` per model. Current endpoint returns `file_exists` but not whether the model is actually loaded in the server runtime. The `/health` endpoint has this info per model but `/models/status` does not cross-reference it.
2. **No server reload after download** — Plan mentioned "server reloads models after download (or shows restart required)". Currently the server loads models only at startup. After downloading via Tauri, user must restart the app.

### Assessment
Functionally complete for the user-facing experience. The two remaining items are quality-of-life improvements.

---

## Phase 4: I/O Sources — PARTIAL (~30%)

### Completed
- **Camera refresh** — `POST /cameras/refresh` endpoint + "Refresh" button in ControlsPanel
- **Resolution selector** — dropdown (480p/720p/1080p) in ControlsPanel, posts to `POST /settings`

### Not Started
1. **`VideoSource` enum in AppState** — Not implemented. No `VideoSource::File(PathBuf)` variant.
2. **Video file input** — No endpoint for `POST /source/video`. No Tauri file picker integration.
3. **Recording output** — No `POST /recording/start` or `/recording/stop` endpoints. No `VideoWriter` integration.
4. **Recording indicator in UI** — No recording controls in frontend.

### Assessment
Only camera refresh and resolution selector are done. The core features (video file input, recording) are entirely missing.

---

## Phase 5: Server Mode — PARTIAL (~60%)

### Completed
- **`--remote` CLI flag** — `parse_remote_flag()` in main.rs, binds `0.0.0.0:8008` when set
- **API token generation** — `generate_token()` creates hex token, stored in `ServerState.api_token`
- **CORS for remote** — `CorsLayer::new().allow_origin(Any)` when remote=true
- **UI display** — ControlsPanel shows server mode info (Local/Remote badge, bind address, token + copy button)
- **ServerState fields** — `remote_mode`, `bind_address`, `api_token` in `ServerState`

### Not Started
1. **Auth middleware** — No actual middleware checking `X-Deep-Forge-Token` header. Token is generated and displayed but never validated on incoming requests. Remote connections are completely unauthenticated.
2. **Token persistence** — Plan called for storing token in `~/.deep-forge/api-token`. Current: token is ephemeral (regenerated every server start).
3. **UI toggle to restart sidecar** — No "Server Mode" toggle in settings. The `--remote` flag must be passed manually (or via Tauri sidecar args). No dynamic switching.
4. **Connected clients count** — Not displayed in metrics panel.

### Assessment
The plumbing (flag, token gen, CORS, UI display) is in place but the security layer (auth middleware) is missing. This is a **security gap** if `--remote` is used in production.

---

## Priority Ordering for Remaining Work

### P0 — Security (before any release with --remote)
1. **Auth middleware for remote mode** — Validate `X-Deep-Forge-Token` header on all non-localhost requests. Skip for `127.0.0.1` / `::1` origins. ~2h
2. **Token persistence** — Write/read from `~/.deep-forge/api-token` so remote clients don't need new tokens on restart. ~1h

### P1 — Core Functionality Gaps
3. **Video file input** — `VideoSource` enum, `POST /source/video` endpoint, Tauri file picker. ~4h
4. **Model hot-reload after download** — Signal dlc-server to reload a specific model without full restart (or at minimum show "restart required" banner). ~3h
5. **`loaded` field in `/models/status`** — Cross-reference with Models struct lock states. ~30min

### P2 — Quality / Polish
6. **EP runtime name in `/health`** — Query ORT for actual EP name rather than showing config enum. ~1h
7. **ort feature flags audit** — Verify `directml` feature is needed in Cargo.toml or if `ORT_LIB_PATH` alone suffices. Add `half` if FP16 models are used. ~1h
8. **Recording output** — `POST /recording/start|stop`, `VideoWriter`, UI controls. ~4h

### P3 — Nice-to-Have
9. **GPU utilization metric** — Platform-specific GPU% and VRAM display. ~3h
10. **Connected remote clients count** — Track WS connections per remote IP. ~1h
11. **Server mode toggle in UI** — Restart sidecar with/without `--remote`. ~2h

---

## Suggested Improvements Beyond Plan

1. **Source face embedding cache** — Currently re-detects source face every frame in `produce_frame`. Cache the embedding after upload (massive perf win). Already noted in deferred issues.
2. **WebSocket reconnect logic** — Frontend does not auto-reconnect if WS drops. Add exponential backoff reconnect in `use-metrics-ws.ts` and video WS connection.
3. **Error boundaries** — No React error boundary around video/metrics components. A crash in overlay rendering kills the whole app.
4. **Concurrent model downloads** — `useModels` handles one download at a time per model but UI doesn't prevent downloading all simultaneously, which could saturate bandwidth.
5. **Resolution actually applied to camera** — `handleResolutionChange` posts to `/settings` but unclear if `CameraCapture::set_resolution()` is wired to AppState resolution field.

---

## Unresolved Questions

1. Does the `ort` crate's DirectML EP work on Windows without the `directml` Cargo feature flag when `ORT_LIB_PATH` points to DirectML DLLs? (Needs testing on Windows target)
2. Is `generate_token()` using `RandomState` hasher cryptographically sufficient for an API token? (It's not — `RandomState` is SipHash, not a CSPRNG. Should use `rand::thread_rng()` or `uuid::Uuid::new_v4()`)
3. What is the actual FPS with DirectML on Radeon 890M? The deferred issues doc mentions "~6 FPS" — is this post-DirectML or still CPU?

# Phase 4: Input/Output Source Selection

**Priority:** P1
**Effort:** 6h
**Status:** Partial (~30%) — audited 2026-03-29

## Overview

Add video file input, camera refresh, and recording output. Virtual camera deferred to v2.

## Implementation Steps

### Step 1: Source type enum in AppState

```rust
pub enum VideoSource {
    Camera(u32),
    File(PathBuf),
}
```

### Step 2: Video file input via Tauri file picker

Use `@tauri-apps/plugin-dialog` for native file dialog. Send selected path to `POST /source/video` endpoint. OpenCV `VideoCapture` accepts file paths with same API.

### Step 3: Camera refresh button

Add `POST /cameras/refresh` that re-probes via `spawn_blocking`. Frontend adds "Refresh" icon button next to camera dropdown.

### Step 4: Recording output

Add `POST /recording/start` and `POST /recording/stop` endpoints. Use OpenCV `VideoWriter` to write MP4. Display recording indicator + duration in UI.

### Step 5: Output resolution selector

Dropdown: 480p / 720p / 1080p. Sets camera resolution via `CameraCapture::set_resolution()`.

## Todo

- [ ] Add `VideoSource` enum to AppState
- [ ] Add video file input endpoint + Tauri file picker
- [x] Add camera refresh endpoint (`POST /cameras/refresh`)
- [ ] Add recording start/stop endpoints
- [x] Add resolution selector (in ControlsPanel, posts to /settings)
- [ ] Update frontend with file input and recording controls

## Success Criteria

- Can process video files (not just live camera)
- Camera list refreshes on button click
- Can record processed output to MP4
- Resolution selector works

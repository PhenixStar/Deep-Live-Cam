# Phase 1: Camera Selection + Face Enhancer Toggles

**Effort:** 4h (2h camera + 2h enhancers)
**Teams:** Team 1 (Frontend) + Team 2 (Backend)
**Depends on:** Nothing (first phase)

---

## Feature 1: Camera Selection Dropdown

### Problem

`server.py` hardcodes `cv2.VideoCapture(0)`. Users with multiple cameras (USB webcam, laptop built-in, virtual cam) cannot choose which to use.

### Existing Code to Reuse

- `modules/ui.py:get_available_cameras()` (lines 1013-1072) -- platform-aware camera enumeration already exists
  - Windows: `FilterGraph().get_input_devices()` via pygrabber
  - Linux: probes indices 0-9 with `cv2.VideoCapture(i).isOpened()`
  - macOS: static `[0, 1]` to avoid OBSENSOR SIGSEGV
- `modules/video_capture.py:VideoCapturer` -- cross-platform capture with DirectShow fallback on Windows
- `cv2_enumerate_cameras` already in `requirements.txt` (unused in server.py but available)

### Backend Changes (Team 2)

#### Step 1: Extract camera enumeration into a shared utility

Create `modules/camera_utils.py` by extracting `get_available_cameras()` from `modules/ui.py`. The function is currently embedded in a 1000+ line tkinter UI file; it has no tkinter dependencies and can be moved cleanly.

```python
# modules/camera_utils.py
# Extract get_available_cameras() from modules/ui.py lines 1013-1072
# Returns: (list[int], list[str]) -- indices and display names
```

**CCS Delegation:** mmu agent -- pure extraction, no logic changes.

#### Step 2: Add GET /cameras endpoint to server.py

```python
@app.get("/cameras")
async def list_cameras():
    """List available cameras with index and display name."""
    indices, names = get_available_cameras()
    return {
        "cameras": [
            {"index": i, "name": n}
            for i, n in zip(indices, names)
        ]
    }
```

**CCS Delegation:** mmu agent -- straightforward endpoint.

#### Step 3: Add POST /camera/{index} to switch active camera

Add shared state for active camera index. Modify `/ws/video` to use it.

```python
# Shared state
_active_camera_index: int = 0
_camera_lock = threading.Lock()

@app.post("/camera/{index}")
async def set_camera(index: int):
    """Switch the active camera. Takes effect on next WS connection or reconnect."""
    global _active_camera_index
    indices, _ = get_available_cameras()
    if index not in indices:
        return JSONResponse(status_code=400, content={"error": f"Camera {index} not available"})
    with _camera_lock:
        _active_camera_index = index
    return {"status": "ok", "camera_index": index}
```

Modify `video_stream()` to read `_active_camera_index` instead of hardcoded `0`:
```python
# In video_stream():
with _camera_lock:
    cam_idx = _active_camera_index
cap = cv2.VideoCapture(cam_idx)
```

**Decision -- hot-switch vs reconnect:** Switching camera mid-stream requires releasing the old `VideoCapture` and opening a new one. Simplest approach: client disconnects WS, calls `POST /camera/{index}`, reconnects WS. The WS handler already creates a fresh `VideoCapture` on each connection. No need for complex mid-stream switching in v1.

**CCS Delegation:** mmu agent for the endpoint. Team 2 Leader reviews the thread-safety of `_active_camera_index` access (it's a single int assignment, atomic in CPython, but lock is defensive).

#### Step 4: Update import in server.py

```python
from modules.camera_utils import get_available_cameras
```

**CCS Delegation:** mmu agent.

### Frontend Changes (Team 1)

#### Step 5: Add camera state and fetch hook

```typescript
// In App.tsx
const [cameras, setCameras] = useState<{index: number; name: string}[]>([]);
const [selectedCamera, setSelectedCamera] = useState<number>(0);

useEffect(() => {
  fetch(`${API_BASE}/cameras`)
    .then(res => res.json())
    .then(data => setCameras(data.cameras))
    .catch(() => setError("Failed to load cameras"));
}, []);
```

**CCS Delegation:** mmu agent.

#### Step 6: Add dropdown in controls section

```tsx
<div className="camera-select">
  <label>Camera</label>
  <select
    value={selectedCamera}
    onChange={async (e) => {
      const idx = Number(e.target.value);
      const res = await fetch(`${API_BASE}/camera/${idx}`, { method: "POST" });
      if (res.ok) {
        setSelectedCamera(idx);
        // Reconnect to use new camera
        if (wsRef.current) { disconnect(); setTimeout(connect, 300); }
      }
    }}
  >
    {cameras.map(c => (
      <option key={c.index} value={c.index}>{c.name}</option>
    ))}
  </select>
</div>
```

Place inside `<section className="controls">` after the source-face upload div.

**CCS Delegation:** mmu agent for JSX. Team 1 Leader for CSS styling decisions.

#### Step 7: Add CSS for camera dropdown

Style `.camera-select` consistent with existing `.source-face` and `.actions` sections.

**CCS Delegation:** mmu agent.

---

## Feature 2: Face Enhancer Toggles

### Problem

Face enhancers (GFPGAN, GPEN-256, GPEN-512) exist as processor modules but are not exposed via the server API. The WS video loop only runs `face_swapper`.

### Existing Code to Reuse

- `modules/globals.py` line 63: `fp_ui` dict already defines `face_enhancer`, `face_enhancer_gpen256`, `face_enhancer_gpen512` as boolean toggles
- `modules/processors/frame/core.py:set_frame_processors_modules_from_ui()` -- reads `fp_ui` and hot-adds/removes processor modules from `FRAME_PROCESSORS_MODULES`
- Each enhancer module (`face_enhancer.py`, `face_enhancer_gpen256.py`, `face_enhancer_gpen512.py`) implements the standard `process_frame(source_face, temp_frame) -> Frame` interface

### Backend Changes (Team 2)

#### Step 8: Add POST /settings endpoint

```python
@app.post("/settings")
async def update_settings(settings: dict):
    """Toggle face enhancers on/off. Keys: face_enhancer, face_enhancer_gpen256, face_enhancer_gpen512."""
    valid_keys = {"face_enhancer", "face_enhancer_gpen256", "face_enhancer_gpen512"}
    for key, value in settings.items():
        if key in valid_keys and isinstance(value, bool):
            globals.fp_ui[key] = value
    return {"status": "ok", "fp_ui": globals.fp_ui}
```

**CCS Delegation:** mmu agent.

#### Step 9: Add GET /settings to read current state

```python
@app.get("/settings")
async def get_settings():
    return {"fp_ui": globals.fp_ui, "frame_processors": globals.frame_processors}
```

**CCS Delegation:** mmu agent.

#### Step 10: Integrate enhancers into WS video loop

Currently `video_stream()` only calls `face_swapper`. Modify to use the processor framework:

```python
# In video_stream(), replace the manual face_swapper block with:
frame_processors = get_frame_processors_modules(globals.frame_processors)
# ... inside the while loop:
if source is not None:
    for processor in frame_processors:
        frame = processor.process_frame(source, frame)
```

This already works because:
- `get_frame_processors_modules()` calls `set_frame_processors_modules_from_ui()` which reads `fp_ui`
- Each enhancer's `process_frame()` takes `(source_face, frame)` and returns the enhanced frame
- The swapper runs first (if present), then enhancers run on the swapped frame

**Important:** The existing WS loop calls `get_frame_processors_modules()` once before the while loop. Move it inside the loop so fp_ui changes take effect immediately:

```python
while True:
    ret, frame = cap.read()
    if not ret: ...

    # Re-fetch processors each frame to pick up fp_ui changes
    frame_processors = get_frame_processors_modules(globals.frame_processors)

    with _source_face_lock:
        source = _source_face

    if source is not None:
        for processor in frame_processors:
            frame = processor.process_frame(source, frame)
    ...
```

**Performance note:** `get_frame_processors_modules()` is cheap -- it checks a list and a dict, no I/O. Safe to call per-frame.

**CCS Delegation:** Team 2 Leader implements this step (touches the critical WS loop, needs careful review). Not delegated to mmu.

### Frontend Changes (Team 1)

#### Step 11: Add enhancer state

```typescript
const [enhancers, setEnhancers] = useState({
  face_enhancer: false,
  face_enhancer_gpen256: false,
  face_enhancer_gpen512: false,
});

// Fetch initial state on mount
useEffect(() => {
  fetch(`${API_BASE}/settings`)
    .then(res => res.json())
    .then(data => setEnhancers(data.fp_ui))
    .catch(() => {});
}, []);
```

**CCS Delegation:** mmu agent.

#### Step 12: Add toggle switches in sidebar

```tsx
<div className="enhancers">
  <label>Face Enhancers</label>
  {[
    { key: "face_enhancer", label: "GFPGAN" },
    { key: "face_enhancer_gpen256", label: "GPEN-256" },
    { key: "face_enhancer_gpen512", label: "GPEN-512" },
  ].map(({ key, label }) => (
    <label key={key} className="toggle">
      <input
        type="checkbox"
        checked={enhancers[key as keyof typeof enhancers]}
        onChange={async (e) => {
          const newState = { ...enhancers, [key]: e.target.checked };
          setEnhancers(newState);
          await fetch(`${API_BASE}/settings`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ [key]: e.target.checked }),
          });
        }}
      />
      {label}
    </label>
  ))}
</div>
```

Place after the camera dropdown in `<section className="controls">`.

**CCS Delegation:** mmu agent for JSX + CSS.

---

## API Contract (Define First -- Unblocks Team 1)

```yaml
GET /cameras
  Response: { cameras: [{ index: int, name: string }] }

POST /camera/{index}
  Response: { status: "ok", camera_index: int }
  Error 400: { error: "Camera {index} not available" }

GET /settings
  Response: { fp_ui: { face_enhancer: bool, face_enhancer_gpen256: bool, face_enhancer_gpen512: bool }, frame_processors: [string] }

POST /settings
  Body: { face_enhancer?: bool, face_enhancer_gpen256?: bool, face_enhancer_gpen512?: bool }
  Response: { status: "ok", fp_ui: { ... } }
```

**Action:** Team 2 Leader writes this contract as first deliverable. Team 1 mocks against it immediately.

---

## CCS Delegation Map

| Step | Task | Assignee | Rationale |
|------|------|----------|-----------|
| 1 | Extract camera_utils.py | mmu | Pure file extraction |
| 2 | GET /cameras endpoint | mmu | Simple endpoint |
| 3 | POST /camera/{index} endpoint | mmu | Simple endpoint |
| 4 | Update server.py imports | mmu | One-line change |
| 5 | Camera state + fetch hook | mmu | Standard React state |
| 6 | Camera dropdown JSX | mmu | Standard select component |
| 7 | Camera dropdown CSS | mmu | Style matching |
| 8 | POST /settings endpoint | mmu | Simple endpoint |
| 9 | GET /settings endpoint | mmu | Simple endpoint |
| 10 | Integrate enhancers into WS loop | **Team 2 Leader** | Critical path, thread safety |
| 11 | Enhancer state hooks | mmu | Standard React state |
| 12 | Enhancer toggle JSX + CSS | mmu | Standard checkbox components |

---

## Success Criteria

- [ ] `GET /cameras` returns at least one camera on test machine
- [ ] Selecting a different camera in dropdown causes video stream to switch (disconnect + reconnect)
- [ ] Toggling GFPGAN on with a source face loaded visibly enhances face quality in stream
- [ ] Toggling GPEN-256 on applies GPEN-256 enhancement
- [ ] Toggling GPEN-512 on applies GPEN-512 enhancement
- [ ] Multiple enhancers can be active simultaneously (stacked pipeline)
- [ ] Toggling enhancer off mid-stream removes the enhancement on subsequent frames
- [ ] No crash on macOS camera enumeration (static list, no probe)
- [ ] Error state shown if no cameras detected

## Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| macOS SIGSEGV on camera probe | App crash | Existing pattern: static [0,1] on Darwin |
| Enhancer model files missing | Enhancer fails silently | `pre_check()` returns False; surface error in GET /settings response |
| Multiple enhancers slow down FPS | Poor UX | Warn in UI if >1 enhancer active; show FPS counter already exists |
| `get_frame_processors_modules()` caches stale list | Toggles don't take effect | Already handled: `set_frame_processors_modules_from_ui()` syncs from `fp_ui` |

---

## Completion Status

**Phase 1 COMPLETE as of 2026-03-28**

All 12 implementation steps delivered:

### Backend Steps
1. **Extracted camera_utils.py** — `get_available_cameras()` extracted from `modules/ui.py` to `modules/camera_utils.py` (no tkinter deps)
2. **GET /cameras endpoint** — Returns list of available cameras with index and display name
3. **POST /camera/{index} endpoint** — Switches active camera with thread-safe shared state + lock
4. **Updated server.py imports** — Added import for `get_available_cameras`

### Frontend Steps
5. **Camera state hooks** — Added state for cameras list and selectedCamera, fetch hook on mount
6. **Camera dropdown JSX** — Added select control in controls section with onChange handler for camera switch
7. **Camera dropdown CSS** — Styled `.camera-select` consistent with existing UI components

### Enhancer Integration Steps
8. **POST /settings endpoint** — Toggles face_enhancer, face_enhancer_gpen256, face_enhancer_gpen512 in fp_ui dict
9. **GET /settings endpoint** — Returns current fp_ui and frame_processors state
10. **WS loop enhancer integration** — Moved `get_frame_processors_modules()` inside frame loop for per-frame fp_ui re-fetch, generic processor pipeline
11. **Enhancer state hooks** — Added state for all three enhancers, fetch initial state on mount
12. **Enhancer toggle JSX + CSS** — Added checkbox toggles for GFPGAN, GPEN-256, GPEN-512 in controls section

### Quality Checks
- **Build passing** — Python syntax OK, TypeScript typecheck OK
- **Code review fixes applied** — Stale closure fixed in camera select handler, missing try/catch added to settings fetch
- **Pre-existing issues noted** — Upstream hardcoded paths and missing error handling in modules not in scope for this phase

### Dependencies Unblocked
Phase 2 can now begin: has working `/cameras` and `/settings` API endpoints to bundle into sidecar package.

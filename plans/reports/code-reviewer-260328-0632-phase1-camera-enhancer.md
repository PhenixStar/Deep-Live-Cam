# Code Review: Phase 1 -- Camera Selection + Face Enhancer Toggles

**Reviewer**: code-reviewer
**Date**: 2026-03-28
**Scope**: camera_utils.py (new), server.py (modified), App.tsx (modified), styles.css (modified)

---

## Scope

- **Files**: 4 (1 new, 3 modified)
- **Estimated LOC changed**: ~200
- **Focus**: Correctness, thread safety, frontend race conditions, error handling, unused imports
- **Scout findings**: Examined processor interfaces, globals.py, core.py module loading, typing.py

## Overall Assessment

Solid Phase 1 implementation. The extraction of camera utils, the generic processor loop, and the frontend controls are well-structured. However, there are several issues ranging from a **critical thread-safety concern** in the processor module system to medium-priority gaps in error handling and API design.

---

## Critical Issues

### C1. `FRAME_PROCESSORS_MODULES` global list mutated without thread safety (server.py:147, core.py:44-78)

**Problem**: `get_frame_processors_modules()` reads and mutates the global `FRAME_PROCESSORS_MODULES` list in `core.py`. This function is called every frame in the WS loop (server.py:147). Simultaneously, `POST /settings` mutates `globals.fp_ui`, and `set_frame_processors_modules_from_ui()` (called inside `get_frame_processors_modules`) appends to or removes from `FRAME_PROCESSORS_MODULES` based on `fp_ui` state.

FastAPI runs `POST /settings` on the async event loop thread. The WS `/ws/video` handler also runs on the same event loop. Since both are `async` coroutines on the same thread, and `get_frame_processors_modules` is called synchronously (no `await` between the dict mutation and the list iteration), this is **likely safe under single-worker uvicorn** because Python's GIL + single event loop thread means they won't interleave mid-statement.

**However**: If uvicorn is ever run with multiple workers, or if `process_frame` is offloaded to a thread pool (e.g., `run_in_executor`), this becomes a race condition causing `RuntimeError: list changed size during iteration` or processing with a stale/partial module list.

**Impact**: Data corruption or crash under concurrent access.

**Recommendation**: Add a `threading.Lock` around `FRAME_PROCESSORS_MODULES` access in `core.py`, or at minimum document the single-threaded assumption. Example:

```python
_modules_lock = threading.Lock()

def get_frame_processors_modules(frame_processors):
    global FRAME_PROCESSORS_MODULES
    with _modules_lock:
        if not FRAME_PROCESSORS_MODULES:
            for fp in frame_processors:
                FRAME_PROCESSORS_MODULES.append(load_frame_processor_module(fp))
        set_frame_processors_modules_from_ui(frame_processors)
        return list(FRAME_PROCESSORS_MODULES)  # return a snapshot
```

### C2. `load_frame_processor_module` calls `sys.exit()` on failure (core.py:32,37,40)

**Problem**: If an invalid processor name somehow reaches `load_frame_processor_module` outside the `set_frame_processors_modules_from_ui` path (which catches `SystemExit`), it kills the entire server process. The initial load path in `get_frame_processors_modules` (lines 47-50) does **not** catch `SystemExit`.

**Impact**: Server crash on startup if `globals.frame_processors` contains a typo or missing module.

**Recommendation**: Wrap the initial load in a try/except or convert `sys.exit()` to raising a proper exception.

---

## High Priority

### H1. `process_frame()` on face_swapper is marked DEPRECATED (face_swapper.py:375-378)

**Problem**: The server's WS loop calls `processor.process_frame(source, frame)` for all processors. The face_swapper's `process_frame` is explicitly marked `DEPRECATED / SIMPLER VERSION`. It does not handle `map_faces` mode or the `simple_map`/`source_target_map` features that `process_frame_v2` supports.

**Impact**: Users who need multi-face mapping won't get it through the Tauri app. This is acceptable for Phase 1 (single source face), but creates a functional gap versus the tkinter UI.

**Recommendation**: Document this as a known limitation. In Phase 2, consider using `process_frame_v2` when `map_faces` is enabled, with appropriate argument adaptation.

### H2. Blocking OpenCV calls in async WS handler (server.py:130,141)

**Problem**: `cv2.VideoCapture(cam_idx)` (line 130), `cap.read()` (line 141), and all `process_frame()` calls are blocking I/O/CPU operations running directly in an `async` coroutine. This blocks the entire event loop, preventing FastAPI from handling other HTTP requests (including `POST /settings` and `POST /camera/{index}`) while a frame is being processed.

**Impact**: Frontend requests to toggle enhancers or switch cameras will queue behind frame processing. With heavy enhancers (GFPGAN), a single frame can take 50-200ms, causing noticeable UI lag on settings changes.

**Recommendation**: Offload frame capture and processing to `asyncio.get_event_loop().run_in_executor(None, ...)`. This also means C1 above becomes more urgent.

```python
loop = asyncio.get_event_loop()
ret, frame = await loop.run_in_executor(None, cap.read)
```

### H3. Camera enumeration is blocking and slow (server.py:86,99)

**Problem**: `GET /cameras` calls `get_available_cameras()` which probes up to 10 camera indices via `cv2.VideoCapture`. On Linux, each failed probe can take 1-3 seconds (timeout). This runs in the async handler, blocking the event loop for potentially 30 seconds.

**Impact**: Frontend mount blocks; all other requests stall during camera enumeration.

**Recommendation**: Run in executor and/or cache the result with a TTL:

```python
@app.get("/cameras")
async def list_cameras():
    loop = asyncio.get_event_loop()
    indices, names = await loop.run_in_executor(None, get_available_cameras)
    ...
```

### H4. `POST /settings` accepts raw `dict` with no Pydantic model (server.py:114)

**Problem**: The endpoint signature `async def update_settings(settings: dict)` bypasses FastAPI's request body validation. Any JSON body is accepted. While there is key validation inside the function (line 117-119), there is no feedback when invalid keys are sent -- they are silently ignored.

**Impact**: Debugging difficulty. A typo like `face_enchancer` silently does nothing.

**Recommendation**: Use a Pydantic model:

```python
from pydantic import BaseModel
from typing import Optional

class SettingsUpdate(BaseModel):
    face_enhancer: Optional[bool] = None
    face_enhancer_gpen256: Optional[bool] = None
    face_enhancer_gpen512: Optional[bool] = None
```

---

## Medium Priority

### M1. `POST /camera/{index}` re-enumerates cameras on every call (server.py:99)

**Problem**: `set_camera` calls `get_available_cameras()` to validate the index. On Linux this probes 10 camera indices, adding seconds of latency to a camera switch.

**Recommendation**: Cache camera list at startup or on first `GET /cameras` call, with an explicit refresh endpoint.

### M2. Frontend reconnect uses `setTimeout(connect, 300)` (App.tsx:104)

**Problem**: The 300ms delay is arbitrary. If the backend takes longer to release the camera (some USB cameras need 500ms+), the new WS connection may fail to open the camera.

**Impact**: Intermittent "Cannot open camera" errors on camera switch, especially with USB webcams.

**Recommendation**: Either increase the timeout or implement retry logic with exponential backoff. Better yet, have the server send a "camera ready" event.

### M3. `handleEnhancerToggle` has stale closure over `enhancers` (App.tsx:111-122)

**Problem**: `handleEnhancerToggle` captures `enhancers` via `useCallback(..., [enhancers])`. If two toggles are clicked rapidly, the second click's spread `{ ...enhancers, [key]: checked }` uses a stale snapshot. React batches state updates, so the first toggle's state may not have been committed yet.

**Impact**: Rapidly toggling two different enhancers could cause one toggle to revert.

**Recommendation**: Use functional state update:

```typescript
const handleEnhancerToggle = useCallback(
  async (key: keyof Enhancers, checked: boolean) => {
    setEnhancers((prev) => ({ ...prev, [key]: checked }));
    await fetch(`${API_BASE}/settings`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ [key]: checked }),
    });
  },
  [],  // no dependency on enhancers
);
```

### M4. `@app.on_event("startup")` is deprecated (server.py:53)

**Problem**: FastAPI deprecated `on_event` in favor of lifespan context managers (since Starlette 0.20+).

**Recommendation**: Migrate to lifespan:

```python
from contextlib import asynccontextmanager

@asynccontextmanager
async def lifespan(app):
    _init_providers()
    yield

app = FastAPI(lifespan=lifespan, ...)
```

### M5. Settings endpoint does not trigger `frame_processors` list sync (server.py:113-120)

**Problem**: `POST /settings` only updates `globals.fp_ui[key]`. The `globals.frame_processors` list (which `get_frame_processors_modules` reads on first call) is not updated. The sync happens lazily inside `set_frame_processors_modules_from_ui`, which is called per-frame. This works, but means there is a 1-frame delay between toggling and effect.

**Impact**: Negligible for users (1 frame at 30fps = 33ms), but the architecture is confusing -- two sources of truth (`fp_ui` and `frame_processors`) that eventually converge.

### M6. `GET /settings` returns the raw `globals.fp_ui` dict (server.py:110)

**Problem**: Returns a mutable reference to the global dict. While JSON serialization creates a copy, a future refactor returning this dict directly could leak internal state.

**Recommendation**: Return an explicit copy or use a response model.

### M7. No error handling on enhancer toggle POST failure (App.tsx:115-119)

**Problem**: If the `POST /settings` fetch fails (network error, server error), the frontend state has already been optimistically updated on line 113-114. There's no rollback.

**Recommendation**: Add error handling with rollback:

```typescript
const res = await fetch(...);
if (!res.ok) {
  setEnhancers((prev) => ({ ...prev, [key]: !checked })); // rollback
  setError("Failed to update setting");
}
```

---

## Low Priority

### L1. `camera_utils.py` Linux probe doesn't release on failure (camera_utils.py:58-63)

**Problem**: When `cap.isOpened()` returns `False`, the capture is not explicitly released. While OpenCV should handle this via destructor, explicit release is best practice.

### L2. macOS hardcodes cameras [0, 1] (camera_utils.py:54-55)

**Problem**: A macOS system with no camera or with 3+ cameras gets wrong results.

**Recommendation**: Document this is a known workaround for the OBSENSOR SIGSEGV issue.

### L3. `handleCameraChange` doesn't handle fetch failure (App.tsx:98)

**Problem**: If `POST /camera/{idx}` fails (e.g., network error), the `await fetch(...)` throws, but there's no catch. This will result in an unhandled promise rejection.

### L4. CORS allows all origins (server.py:30)

**Problem**: `allow_origins=["*"]` is appropriate for localhost development but should be tightened for any non-local deployment.

### L5. JPEG quality hardcoded to 80 (server.py:158)

**Problem**: Quality 80 is reasonable for streaming but could be made configurable for bandwidth-constrained scenarios.

---

## Edge Cases Found by Scouting

1. **`process_frame` type mismatch**: `face_swapper.process_frame` types its first arg as `Face` (non-optional), while enhancers type it as `Face | None`. The server passes `_source_face` which could be `None` if no source is uploaded. Calling `face_swapper.process_frame(None, frame)` when `source is not None` is safe because `_source_face` is only set after `get_one_face` succeeds. However, if `_source_face` is somehow corrupted, the swapper will call methods on `None`. The `if source is not None` guard on line 153 protects this path.

2. **`get_frame_processors_modules` initial load path**: On startup, `globals.frame_processors = ["face_swapper"]`. The first WS connection triggers `get_frame_processors_modules(["face_swapper"])`, which calls `load_frame_processor_module("face_swapper")`. If the face_swapper model file is missing, this calls `sys.exit()` and kills the server (see C2).

3. **Camera index 0 default vs. empty camera list**: If `get_available_cameras()` returns `([], ["No cameras found"])`, the default `_active_camera_index = 0` will attempt to open camera 0, which may or may not exist. The WS handler does check `cap.isOpened()` and returns an error, so this is handled.

4. **Frontend `enhancers` state init vs. server state**: On mount, `GET /settings` populates `enhancers` from `data.fp_ui`. If the server's `fp_ui` has additional keys not in the `Enhancers` interface, TypeScript won't complain at runtime, but the checkboxes will only show the three hardcoded keys. This is fine for now but could cause hidden state drift.

5. **`set_frame_processors_modules_from_ui` removes modules from list during iteration**: The function iterates `globals.fp_ui.items()` and mutates `FRAME_PROCESSORS_MODULES` via `.remove()`. Since `fp_ui` and `FRAME_PROCESSORS_MODULES` are different collections, this is safe -- but it does mutate the list that callers are about to iterate over.

---

## Positive Observations

- Clean extraction of `camera_utils.py` with proper platform branching and docstrings
- Good use of locks for `_source_face` and `_active_camera_index` shared state
- The generic `process_frame()` loop is a solid architectural improvement over hardcoded `swap_face()` calls
- Frontend code is well-structured with proper TypeScript interfaces and `useCallback` memoization
- CSS follows existing theme conventions consistently
- Camera error reported to client via WS JSON before closing
- Enhancer key validation with allowlist prevents arbitrary `fp_ui` pollution

---

## Recommended Actions (Priority Order)

1. **[C1]** Add thread safety to `FRAME_PROCESSORS_MODULES` access in `core.py`, or document single-worker assumption
2. **[C2]** Catch `SystemExit` in the initial module load path of `get_frame_processors_modules`
3. **[H2]** Offload blocking OpenCV/ONNX calls to `run_in_executor` in the WS loop
4. **[H3]** Run `get_available_cameras()` in executor; add caching
5. **[H4]** Replace raw `dict` param with Pydantic model on `POST /settings`
6. **[M3]** Fix stale closure in `handleEnhancerToggle` with functional state update
7. **[M7]** Add error handling + rollback for enhancer toggle POST
8. **[M2]** Add retry logic or increase timeout for camera reconnect
9. **[L3]** Add try/catch to `handleCameraChange`
10. **[M4]** Migrate from deprecated `on_event("startup")` to lifespan

---

## Metrics

| Metric | Value |
|--------|-------|
| Type Coverage (TSX) | Good -- interfaces defined, proper typing on handlers |
| Type Coverage (Python) | Partial -- `settings: dict` bypasses typing |
| Test Coverage | Not assessed (no tests in scope) |
| Linting Issues | 0 unused imports detected in changed files |
| Security Issues | CORS wildcard (L4, acceptable for localhost sidecar) |

---

## Unresolved Questions

1. Should the Tauri app ever support `map_faces` mode? If so, `process_frame_v2` integration needs planning.
2. Is multi-worker uvicorn deployment a target? If yes, C1 is critical; if not, document the constraint.
3. Should camera enumeration be cached server-side, or is per-request probing acceptable for the Tauri use case?
4. The face_swapper `process_frame` is deprecated upstream -- is there a plan to migrate to `process_frame_v2`?

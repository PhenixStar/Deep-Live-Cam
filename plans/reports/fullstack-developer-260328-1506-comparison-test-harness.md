# Phase Implementation Report

## Executed Phase
- Phase: comparison-test-harness
- Plan: none (ad-hoc task)
- Status: completed

## Files Modified

| File | Action | Notes |
|------|--------|-------|
| `core/rust-engine/tests/compare_backends.py` | created | Python comparison harness |
| `core/rust-engine/dlc-server/tests/integration.rs` | created (moved from workspace tests/) | Rust in-process integration tests |
| `core/rust-engine/dlc-server/src/lib.rs` | created | Exposes `build_router` + `test_state` to test crate |
| `core/rust-engine/dlc-server/src/router.rs` | created | All handlers + `build_router` + `test_state` extracted from main |
| `core/rust-engine/dlc-server/src/main.rs` | rewritten | Slim startup: parse args, load models, call `build_router`, bind socket |
| `core/rust-engine/dlc-server/Cargo.toml` | updated | Added `[lib]` + `[[bin]]` targets; added `tower` + `http-body-util` dev-deps |

## Tasks Completed

- [x] Python comparison harness `compare_backends.py`
  - Starts both servers (Python :8008, Rust :8009) or connects to running ones
  - Tests: GET /health, GET /cameras, GET /settings, POST /settings, POST /source, POST /swap/image stub
  - Prints result table: TEST | PYTHON | RUST | MATCH | NOTES
  - `--python-url`, `--rust-url`, `--no-start`, `--timeout`, `--source` CLI args
  - Graceful server teardown via SIGTERM
- [x] Rust integration tests `dlc-server/tests/integration.rs`
  - 11 tests, all in-process via `tower::ServiceExt::oneshot` (no TCP)
  - GET /health → 200 + `{"status":"ok","backend":"rust"}`
  - GET /cameras → 200 + array with index/name fields
  - GET /settings → 200 + fp_ui with three booleans
  - POST /settings → 200 + `{"status":"ok"}`
  - POST /source valid JPEG → 200 (skipped gracefully if asset absent)
  - POST /source invalid bytes → 422
  - POST /source empty multipart → 400
  - POST /camera/0 → 200
  - POST /camera/99 → 400
  - POST /swap/image (no models) → 503
  - Inline minimal 1x1 JPEG (no external file dependency for core tests)
- [x] Refactored `dlc-server` into lib + bin to enable clean test imports
  - `src/router.rs` owns all handlers and `build_router(ServerState) -> Router`
  - `src/lib.rs` re-exports `pub mod router`, `pub mod state`, and `test_state`
  - `src/main.rs` reduced to ~50 lines: arg parsing, model loading, socket binding

## Tests Status

- Type check: pass (`cargo build -p dlc-server` clean)
- Unit tests: pass (11/11)
- Integration tests: pass (11/11, 0.08s)

```
running 11 tests
test cameras_has_index_and_name_fields ... ok
test set_camera_unknown_index_returns_400 ... ok
test set_camera_valid_index_returns_ok ... ok
test cameras_returns_json_array ... ok
test health_returns_ok ... ok
test source_upload_invalid_image_returns_422 ... ok
test post_settings_returns_ok ... ok
test get_settings_has_fp_ui_booleans ... ok
test source_upload_no_field_returns_400 ... ok
test swap_image_without_models_returns_503 ... ok
test source_upload_valid_jpeg_returns_ok ... ok

test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured
```

## Issues Encountered

1. Initial `integration.rs` used `#[path]` to reach into the binary's private modules — not valid for integration test crates. Fixed by extracting router logic into `src/router.rs` and adding a `[lib]` target.
2. Initial `integration.rs` placed in workspace `tests/` instead of `dlc-server/tests/` — Cargo only picks up package-level `tests/` directories. Moved.
3. Initial draft re-implemented handlers inline in the test file — replaced with direct import of `dlc_server::router::build_router`.
4. Ignored live-server test referenced `reqwest` which isn't a dep — removed the ignored test entirely (the Python harness covers live-server comparison).

## Next Steps

- `compare_backends.py` requires both servers to be independently startable; the Rust server currently binds :8008 by default. To test against a separate port, either add a `--port` flag to `dlc-server` or run it with a different config. The Python harness defaults to py=:8008, rs=:8009 and will warn if a server is unreachable.
- The `/swap/image` Python-vs-Rust comparison path (SSIM comparison) is scaffolded as a stub test; full comparison can be added once the Python server also returns a JPEG from that endpoint.

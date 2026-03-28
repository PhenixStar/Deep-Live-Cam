# Plan Report: Deep-Live-Cam Next Phases

**Date:** 2026-03-28 04:38 UTC
**Plan dir:** `plans/260328-0424-deep-wcam-next-phases/`

## Deliverables

| File | Purpose |
|------|---------|
| `plan.md` | Master plan with both approaches, recommendation, CCS delegation |
| `phase-01-upstream-prs.md` | WS1: Cherry-pick 6 PRs (both approaches) |
| `phase-02-rust-core.md` | WS2: Rust rewrite, Approach A (Weeks 1-6) |
| `phase-03-tauri-app.md` | WS3: Tauri desktop app, Approach A (Weeks 1-6) |
| `phase-02-tauri-app-sequential.md` | WS3: Tauri app, Approach B (Weeks 2-4) |
| `phase-03-rust-migration-sequential.md` | WS2: Rust migration, Approach B (Weeks 5-8) |

## Two Approaches

### Approach A: Parallel Sprint (6 weeks)
- All 3 workstreams run simultaneously
- 15 agents busy for 6 weeks
- Risk: two competing frontends (Tauri + potential Rust/egui)
- Fastest if ort validation passes Week 1

### Approach B: Sequential Pipeline (8 weeks) -- RECOMMENDED
- WS1 (Week 1) -> Tauri+Python v1.0 (Weeks 2-4) -> Rust v2.0 (Weeks 5-8)
- Ship working app in 4 weeks, replace Python backend later
- Python FastAPI server = reference oracle for Rust validation
- Lower risk, better resource utilization

## Key Design Decisions

1. **HTTP API contract** shared between Python (FastAPI) and Rust (axum) backends. Defined in Week 2, honored by both. This is the single integration point that enables drop-in replacement.

2. **WebSocket binary frames** for 30fps streaming (not base64, not WebRTC). ~40-70ms latency, sufficient for real-time preview.

3. **python-build-standalone + PyInstaller** for sidecar bundling. Fallback: ship Python directory if PyInstaller fails with ONNX Runtime GPU.

4. **ort v2.0 validation gate** in Week 1/5. If inswapper model fails to load, fallback to PyO3 hybrid.

5. **CCS delegation:** Claude for architecture/review, mmhs for implementation, mm for UI/testing. ~30% Claude, ~40% mmhs, ~30% mm across all teams.

## Critical Path Items

- PR #1707 (FP32 fix) must land before anything else -- V100 NaN stability
- ort + inswapper_128.onnx compatibility -- no public Rust examples exist
- PyInstaller + ONNX Runtime GPU bundling -- known to be fragile
- HTTP API contract -- must be defined early, shared across teams

## Unresolved Questions

1. InsightFace buffalo_l license for compiled distribution
2. ort + inswapper_128 compatibility (no public examples)
3. CUDA 12.0 bundling strategy (bundle vs require user install)
4. Code signing costs (macOS $99/yr, Windows $200-400/yr)
5. uv migration PR #1688 (blocked on upstream Pillow fix)
6. SharedArrayBuffer for zero-copy streaming (v1.1 investigation)

# Deep Forge v0.3 — GPU Acceleration, Enhanced UI, Server Mode

**Created:** 2026-03-29
**Status:** Planning
**Priority:** High

## Overview

Transform Deep Forge from a functional prototype (CPU inference, minimal UI) into a production-quality desktop app with GPU acceleration, comprehensive debug/monitoring controls, model management, and remote server mode.

## Phases

| Phase | Description | Priority | Effort | Status |
|-------|-------------|----------|--------|--------|
| [Phase 1](phase-01-directml-gpu.md) | DirectML GPU acceleration | P0 | 8h | Pending |
| [Phase 2](phase-02-enhanced-frontend.md) | Debug overlay, metrics, source scoring | P0 | 10h | Pending |
| [Phase 3](phase-03-model-management.md) | Model status, download, configuration | P1 | 8h | Pending |
| [Phase 4](phase-04-io-sources.md) | Input/output source selection | P1 | 6h | Pending |
| [Phase 5](phase-05-server-mode.md) | Remote server mode with API token | P2 | 5h | Pending |

## Dependencies

```
Phase 1 (GPU) ──┐
                 ├──> Phase 2 (UI) ──> Phase 3 (Models) ──> Phase 5 (Server)
                 │                 └──> Phase 4 (I/O)
```

Phase 1 is independent and highest priority. Phase 2 depends on Phase 1 (needs GPU metrics). Phases 3-5 can follow in any order.

## Research Reports

- [DirectML + ort crate](../reports/researcher-260329-directml-ort.md)
- [UI architecture patterns](../reports/researcher-260329-ui-architecture.md)
- [AMD APU inference approaches](../reports/researcher-260328-1257-amd-apu-npu-inference.md)

## Key Decisions

1. **DirectML over ROCm** — DirectML works on all Windows GPUs (AMD/NVIDIA/Intel). ROCm unstable on iGPU.
2. **NuGet DLLs over source build** — Extract from `Microsoft.ML.OnnxRuntime.DirectML` NuGet. No multi-hour ORT compile.
3. **std::sync::Mutex for models** — Already switched. Required for spawn_blocking compatibility.
4. **Tauri commands for system metrics** — Not HTTP. Use `sysinfo` crate in Tauri shell.
5. **Separate metrics WS endpoint** — `/ws/metrics` pushes JSON; `/ws/video` stays binary-only.

---
title: "Deep-Live-Cam: Cross-Platform App & Upstream Integration"
description: "3-workstream plan: upstream PRs, Rust rewrite, Tauri desktop app"
status: pending
priority: P1
effort: 6w
branch: main
tags: [deep-live-cam, rust, tauri, cross-platform, upstream-prs]
created: 2026-03-28
---

# Deep-Live-Cam: Next Phases Implementation Plan

## Current State

- **Codebase:** 3,200 LOC Python, 16 core modules + 7 frame processors
- **Stack:** ONNX Runtime, InsightFace, OpenCV, CustomTkinter, FFmpeg
- **Hardware:** 4x V100-DGXS-32GB, CUDA 12.0
- **Fork status:** PhenixStar/Deep-Live-Cam, 11 commits ahead of upstream
- **Already cherry-picked:** PRs #1661, #1657, #1673, #1655, #1668, #1672, #1681, #1676

## Team Structure

| Role | Scope | Max CCS Agents | Agent Type |
|------|-------|:-:|---|
| **Team 1 Leader** | PR Integration + Code Quality | 5 | mmhs for cherry-picks, mm for validation |
| **Team 2 Leader** | Rust Core / Tauri Backend | 5 | Claude for architecture, mmhs for implementation |
| **Team 3 Leader** | Frontend / Testing / Distribution | 5 | mm for UI, mmhs for build scripts |

---

## Approach A: "Parallel Sprint" (Aggressive)

**Timeline:** 6 weeks
**Parallelism:** All 3 workstreams simultaneous after Week 1

```
Week 1:  [WS1: Upstream PRs ━━━━━━━━]  [WS2: Rust scaffold ──]  [WS3: Tauri scaffold ──]
Week 2:  [WS1: done ✓]                 [WS2: Face detect ━━━━]  [WS3: Sidecar + IPC ━━━]
Week 3:                                 [WS2: Face swap ━━━━━━]  [WS3: Web UI ━━━━━━━━━━]
Week 4:                                 [WS2: Enhancers ━━━━━━]  [WS3: Streaming ━━━━━━━]
Week 5:                                 [WS2: Pipeline ━━━━━━━]  [WS3: Installers ━━━━━━]
Week 6:                                 [WS2: Integration ━━━━]  [WS3: Auto-update ━━━━━]
```

### WS1: Upstream PR Integration (Week 1)

**Owner:** Team 1 Leader
**Details:** [phase-01-upstream-prs.md](phase-01-upstream-prs.md)

Cherry-pick 8 PRs in dependency order:
1. **Batch 1 (Security + Stability):** #1682, #1707, #1667
2. **Batch 2 (Code Quality):** #1683, #1684, #1685
3. **Batch 3 (Monitor):** #1688 (defer), #1620/#1616 (test-if-needed)

**Success Criteria:**
- All Batch 1+2 PRs merged without regressions
- V100 FP32 inference confirmed stable (no NaN)
- Zero new security warnings from SSL/checksum changes

### WS2: Rust Core Rewrite (Weeks 1-6)

**Owner:** Team 2 Leader
**Details:** [phase-02-rust-core.md](phase-02-rust-core.md)

Progressive rewrite of Python inference pipeline to Rust:
1. **Week 1:** Project scaffold, `ort` v2.0 session loading, model I/O validation
2. **Weeks 2-3:** Face detection (SCRFD via ort), face swap (inswapper_128)
3. **Week 4:** Face enhancer (GFPGAN/GPEN), mouth masking
4. **Week 5:** Async pipeline (tokio), camera capture (nokhwa), virtual camera
5. **Week 6:** Integration tests, benchmarks vs Python, binary packaging

**Success Criteria:**
- Rust binary processes single image face swap correctly
- CUDA provider works on V100
- Latency <= Python baseline (33ms/frame target)
- Binary size < 80MB (excluding models)

### WS3: Tauri Desktop App (Weeks 1-6)

**Owner:** Team 3 Leader
**Details:** [phase-03-tauri-app.md](phase-03-tauri-app.md)

Wrap Python backend in Tauri v2 with modern web UI:
1. **Week 1:** Tauri v2 scaffold, sidecar config, FastAPI server skeleton
2. **Week 2:** Python sidecar bundling (python-build-standalone + PyInstaller)
3. **Week 3:** React/Svelte UI (tabs: Image, Video, Live, Settings)
4. **Week 4:** WebSocket binary frame streaming at 30fps
5. **Week 5:** Platform installers (MSI, DMG, AppImage)
6. **Week 6:** Auto-update, system tray, code signing

**Success Criteria:**
- Installable app on Windows/Linux/macOS
- 30fps live preview with <100ms latency
- Auto-update from GitHub Releases
- Bundle size < 300MB (excluding CUDA runtime)

### Approach A: Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|:-:|:-:|---|
| Rust + Tauri compete for Team 2/3 time | High | Medium | Clear ownership, no cross-workstream dependencies until Week 6 |
| `ort` crate issues with inswapper model | Medium | High | Week 1 validation gate; fallback to PyO3 wrapper |
| Tauri sidecar GPU detection failures | Medium | Medium | CPU fallback default; GPU as optional download |
| Cherry-pick conflicts from Batch 2 | Low | Low | Apply after #1707; manual resolution |
| Two parallel UIs (Tauri + Rust/egui) | High | Medium | Tauri ships first; Rust UI deferred to post-v1 |

**Key Risk:** Running Rust rewrite AND Tauri app in parallel means two competing frontends. Mitigation: Tauri app uses Python backend (ships in Week 6). Rust core replaces Python backend later, keeping Tauri frontend.

---

## Approach B: "Sequential Pipeline" (Conservative)

**Timeline:** 8 weeks
**Strategy:** Ship Tauri+Python app first, then incrementally replace Python with Rust

```
Week 1:  [WS1: Upstream PRs ━━━━━━━━]
Week 2:  [WS3: Tauri scaffold + sidecar ━━━━━━━━━━━━━━━━━━━━━━━]
Week 3:  [WS3: Web UI + WebSocket streaming ━━━━━━━━━━━━━━━━━━━]
Week 4:  [WS3: Installers + auto-update + code signing ━━━━━━━━]  → SHIP v1.0
Week 5:  [WS2: Rust scaffold + ort validation ━━━━━━━━━━━━━━━━━]
Week 6:  [WS2: Face detect + swap in Rust ━━━━━━━━━━━━━━━━━━━━━]
Week 7:  [WS2: Enhancers + pipeline + virtual camera ━━━━━━━━━━]
Week 8:  [WS2: Replace Python sidecar with Rust binary ━━━━━━━━]  → SHIP v2.0
```

### WS1: Upstream PR Integration (Week 1)

**Identical to Approach A.** See [phase-01-upstream-prs.md](phase-01-upstream-prs.md).

### WS3: Tauri Desktop App (Weeks 2-4)

**Owner:** Team 3 Leader (primary) + Team 2 Leader (Rust/Tauri core)
**Details:** [phase-02-tauri-app-sequential.md](phase-02-tauri-app-sequential.md)

Compressed 3-week sprint to ship v1.0 with Python backend:
1. **Week 2:** Tauri scaffold + python-build-standalone bundling + FastAPI sidecar
2. **Week 3:** React UI + WebSocket binary streaming + face-swap controls
3. **Week 4:** Platform builds (MSI/DMG/AppImage) + auto-update + QA

**Success Criteria:**
- v1.0 installable app shipped by end of Week 4
- Same feature parity as current CustomTkinter UI
- 30fps live preview
- Auto-update enabled

### WS2: Rust Core Migration (Weeks 5-8)

**Owner:** Team 2 Leader (primary) + Team 1 (testing)
**Details:** [phase-03-rust-migration-sequential.md](phase-03-rust-migration-sequential.md)

Incremental replacement of Python sidecar with Rust binary:
1. **Week 5:** Rust project init, ort model loading, SCRFD face detection
2. **Week 6:** Inswapper face swap, embedding extraction, ndarray preprocessing
3. **Week 7:** GFPGAN enhancer, mouth masking, async pipeline, virtual camera
4. **Week 8:** Replace Python sidecar with Rust binary in Tauri app, benchmark, ship v2.0

**Success Criteria:**
- Rust binary is a drop-in replacement for Python sidecar (same FastAPI-compatible HTTP API)
- Performance >= Python baseline
- v2.0 ships with identical UI, Rust backend
- Binary size < 80MB

### Approach B: Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|:-:|:-:|---|
| Tauri v1.0 ships with Python perf limitations | Expected | Low | Acceptable; users get app now, speed later |
| 3-week Tauri sprint too tight | Medium | Medium | Cut to essential features; defer system tray to v1.1 |
| Rust migration takes >4 weeks | High | Medium | Extend to Week 10; ship incremental Rust modules |
| Python sidecar → Rust API incompatibility | Medium | High | Define HTTP API contract in Week 2; both must conform |
| Team idle time (Team 1 done Week 1) | Expected | Low | Team 1 assists Team 3 (testing, CI) in Weeks 2-4 |

---

## CCS Delegation Strategy (Both Approaches)

### Agent Type Selection

| Task Type | Agent | Rationale |
|-----------|-------|-----------|
| Cherry-pick PRs | mmhs | Mechanical git ops, low reasoning |
| Conflict resolution | Claude | Requires codebase understanding |
| Architecture decisions | Claude | Needs context, tradeoff analysis |
| Rust module implementation | mmhs | Follows established patterns |
| React/Svelte components | mm | UI boilerplate, well-known patterns |
| Build scripts (CI/CD) | mmhs | Platform-specific but repetitive |
| Test writing | mm | Follows test patterns |
| Code review | Claude | Needs judgment, security awareness |
| Documentation | mm | Templated, reference-heavy |
| Performance profiling | Claude | Requires analysis, interpretation |

### Per-Team Agent Allocation

**Team 1 (5 agents):**
- Agent 1 (mmhs): Cherry-pick Batch 1 PRs
- Agent 2 (mmhs): Cherry-pick Batch 2 PRs
- Agent 3 (mm): Validate each PR (run tests, check V100)
- Agent 4 (mm): Update docs/CHANGELOG for merged PRs
- Agent 5 (Claude): Resolve merge conflicts, review security PR #1682

**Team 2 (5 agents):**
- Agent 1 (Claude): Architecture design, Cargo.toml, module interfaces
- Agent 2 (mmhs): Implement face detection module (SCRFD + ort)
- Agent 3 (mmhs): Implement face swap module (inswapper + ndarray)
- Agent 4 (mmhs): Implement enhancer + masking modules
- Agent 5 (mm): Integration tests, benchmarks, CI pipeline

**Team 3 (5 agents):**
- Agent 1 (Claude): Tauri architecture, sidecar config, security capabilities
- Agent 2 (mm): React/Svelte UI components
- Agent 3 (mmhs): FastAPI server + WebSocket streaming
- Agent 4 (mmhs): Build scripts (PyInstaller, python-build-standalone)
- Agent 5 (mm): Platform installer testing, auto-update config

---

## Dependency Graph

```
                     ┌─────────────────────┐
                     │  WS1: Upstream PRs   │
                     │  (Week 1, Team 1)    │
                     └──────────┬──────────┘
                                │
               ┌────────────────┼────────────────┐
               ▼                ▼                ▼
    ┌──────────────────┐  ┌──────────────┐  ┌──────────────────┐
    │ WS2: Rust Core   │  │  Shared API  │  │ WS3: Tauri App   │
    │ (Weeks 2-6/5-8)  │◄─┤  Contract    ├─►│ (Weeks 2-6/2-4)  │
    │ Team 2           │  │  (Week 2)    │  │ Team 3           │
    └────────┬─────────┘  └──────────────┘  └────────┬─────────┘
             │                                       │
             ▼                                       ▼
    ┌──────────────────┐                   ┌──────────────────┐
    │ Rust replaces    │                   │ Ship Tauri v1.0  │
    │ Python sidecar   │                   │ (Python backend) │
    └──────────────────┘                   └──────────────────┘
```

**Critical Path:**
- Approach A: WS1 → WS2 (ort validation gate in Week 1) → Integration (Week 6)
- Approach B: WS1 → WS3 (ship v1.0 Week 4) → WS2 (ship v2.0 Week 8)

**Shared Dependency:** HTTP API contract must be defined in Week 2 and honored by both Python (FastAPI) and Rust (axum/actix) backends. This is the single integration point.

---

## Recommendation: Approach B ("Sequential Pipeline")

**Choose Approach B.** Rationale:

1. **Ship early, learn fast.** A Tauri+Python app in 4 weeks gives real users and real feedback before committing to a Rust rewrite. The Rust rewrite is high-effort, high-risk work that benefits from user-validated requirements.

2. **De-risked Rust migration.** By defining the HTTP API contract during the Tauri phase, the Rust binary becomes a drop-in replacement. No frontend changes needed for v2.0.

3. **Team utilization is better.** In Approach A, Team 2 (Rust) and Team 3 (Tauri) work independently but produce two competing systems. In Approach B, Team 2 builds on Team 3's foundation (same Tauri frontend, same API contract).

4. **The Python backend is fine for now.** ML inference is the bottleneck, and that's ONNX Runtime (compiled C++) in both Python and Rust. The orchestration overhead Python adds is ~5-10ms/frame. Users won't notice until scale matters.

5. **Resource efficiency.** 3 teams x 5 agents = 15 CCS agents. Approach B uses Team 1 for 1 week, then reassigns to QA. Approach A keeps all 15 agents busy for 6 weeks, burning more tokens for marginal speedup.

6. **V100 FP32 fix (#1707) must land first.** Both approaches need stable Python inference as the baseline. Approach B makes this explicit (Week 1 gate).

**When to pick Approach A instead:**
- If time-to-market for Rust binary is critical (competitive pressure)
- If the team has prior `ort` crate experience (reduces Week 1 risk)
- If Tauri app is considered throwaway (not shipping to users)

---

## Phase Files

| File | Covers |
|------|--------|
| [phase-01-upstream-prs.md](phase-01-upstream-prs.md) | WS1: Cherry-pick plan (both approaches) |
| [phase-02-rust-core.md](phase-02-rust-core.md) | WS2: Rust rewrite (Approach A, Weeks 1-6) |
| [phase-03-tauri-app.md](phase-03-tauri-app.md) | WS3: Tauri desktop app (Approach A, Weeks 1-6) |
| [phase-02-tauri-app-sequential.md](phase-02-tauri-app-sequential.md) | WS3: Tauri app (Approach B, Weeks 2-4) |
| [phase-03-rust-migration-sequential.md](phase-03-rust-migration-sequential.md) | WS2: Rust migration (Approach B, Weeks 5-8) |

---

## Unresolved Questions

1. **InsightFace license:** buffalo_l models are non-commercial. Does distributing a compiled app with these models require a license? (Blocks both approaches)
2. **ort + inswapper_128 compatibility:** No public Rust examples of running inswapper ONNX models. Week 1 validation gate mitigates this.
3. **CUDA 12.0 bundling:** V100 uses CUDA 12.0. Tauri app bundling CUDA runtime adds ~500MB. Ship without and require user CUDA install, or bundle?
4. **Code signing costs:** macOS notarization requires Apple Developer ($99/yr). Windows code signing requires EV certificate (~$200-400/yr). Who pays?
5. **#1688 (uv migration):** Deferred due to broken Pillow pin. If upstream fixes it, should we adopt uv for the Python sidecar build?
6. **Shared memory streaming:** SharedArrayBuffer for zero-copy frame transfer is fastest but needs specific browser flags. Worth investigating for v1.1?

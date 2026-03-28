# Phase 1: Upstream PR Integration

**Workstream:** WS1
**Owner:** Team 1 Leader
**Timeline:** Week 1 (both approaches)
**Agents:** 5 CCS-delegated

---

## Objective

Cherry-pick remaining valuable PRs from hacksider/Deep-Live-Cam into PhenixStar/Deep-Live-Cam. Stabilize V100 inference (FP32 fix) and harden security (SSL/checksum).

## Pre-Conditions

- Fork is 11 commits ahead of upstream
- Already picked: #1661, #1657, #1673, #1655, #1668, #1672, #1681, #1676
- Running on 4x V100-DGXS-32GB, CUDA 12.0

---

## Steps

### Step 1: Add upstream remote and fetch (Day 1, 1h)

**CCS Agent:** mmhs (Agent 1)
**Command sequence:**
```bash
git remote add upstream https://github.com/hacksider/Deep-Live-Cam.git  # if not already
git fetch upstream
git log --oneline upstream/main --since="2025-11-01" | head -30
```

**Output:** Updated refs for all upstream branches.

---

### Step 2: Cherry-pick Batch 1 — Security + Stability (Day 1, 2h)

**CCS Agent:** mmhs (Agent 1) for picks, Claude (Agent 5) for #1682 review

#### 2a. PR #1682 — Remove SSL bypass, add checksum validation
- **Files:** 2 | +36, -11
- **Touches:** model download logic (face_swapper, face_enhancer)
- **Why first:** Security fix must precede any model downloads
- **Delegation:** Claude reviews diff for correctness (SSL removal, checksum algo)
- **Command:** `git cherry-pick <commit-sha>`
- **Verify:** `python -c "from modules.processors.frame.face_swapper import *; print('OK')"`

#### 2b. PR #1707 — Switch to FP32 model by default
- **Files:** 2 | +19, -4
- **Touches:** `face_swapper.py` (model path), `run.py` (wrapper)
- **Why critical:** Fixes NaN on V100 (non-Tensor-Core architecture)
- **Delegation:** mmhs (mechanical cherry-pick)
- **Verify:** Run face swap on V100, confirm no NaN in output tensor
- **Post-pick:** Validate `inswapper_128.onnx` (FP32) exists in models dir; download if missing

#### 2c. PR #1667 — Fix macOS memory limit (exabytes to gigabytes)
- **Files:** 1 | +1, -1
- **Touches:** `modules/core.py` (macOS guard clause only)
- **Delegation:** mmhs (trivial one-line change)
- **Verify:** No behavior change on Linux (guard clause skips)

**Gate:** All 3 PRs must apply cleanly. If conflict, escalate to Claude (Agent 5).

---

### Step 3: V100 Validation (Day 2, 2h)

**CCS Agent:** mm (Agent 3)

Run end-to-end tests on V100 after Batch 1:
1. **Image mode:** Single face swap, source + target → output
2. **Video mode:** 10-second clip, face swap with GFPGAN enhancement
3. **Live mode:** Webcam capture → virtual camera output, 30 seconds
4. **Check:** No NaN values, no SSL errors, correct model loaded (FP32)

**Validation script:**
```bash
python run.py -s test_assets/source.jpg -t test_assets/target.jpg \
  --execution-provider cuda --frame-processor face_swapper \
  -o /tmp/test_output.jpg
# Verify output is valid image, not black/corrupted
python -c "from PIL import Image; img = Image.open('/tmp/test_output.jpg'); print(f'{img.size} OK')"
```

**Gate:** If V100 NaN persists after #1707, investigate FP16→FP32 model download path. Do not proceed to Batch 2 until stable.

---

### Step 4: Cherry-pick Batch 2 — Code Quality (Day 2-3, 3h)

**CCS Agent:** mmhs (Agent 2)

Apply in order (each depends on previous line numbers):

#### 4a. PR #1683 — Named constants for magic numbers
- **Files:** 1 | +7, -0
- **Risk:** None (additive only)
- **Delegation:** mmhs

#### 4b. PR #1684 — Extract process_frames_io helper
- **Files:** 2 | +29, -19
- **Risk:** Low (internal refactor)
- **Delegation:** mmhs
- **Verify:** `python -c "from modules.processors.frame.core import *; print('OK')"`

#### 4c. PR #1685 — Extract pair-building helpers, reduce nesting
- **Files:** 2 | +97, -93
- **Risk:** Low (pure refactor, no behavior change)
- **Delegation:** mmhs
- **Note:** Must apply AFTER #1707 to avoid rebase churn in face_swapper.py

**Gate:** Run same validation as Step 3 after all Batch 2 picks.

---

### Step 5: Update documentation (Day 3, 1h)

**CCS Agent:** mm (Agent 4)

- Update CHANGELOG with merged PRs
- Update any internal docs referencing FP16 model paths
- Commit with descriptive message listing all cherry-picked PRs

---

### Step 6: Evaluate Batch 3 — Deferred PRs (Day 3, 1h)

**CCS Agent:** Claude (Agent 5)

Review and document decision for:
- **#1688 (uv migration):** Check if Pillow version pin fixed upstream. If yes, queue for Week 2.
- **#1620 (19+ faces UI bug):** Attempt to reproduce. If reproducible, cherry-pick.
- **#1616 (face_outline_points NameError):** Grep codebase for the variable. If referenced, cherry-pick.
- **#1666 (virtual camera):** Already implemented in our fork? Check `virtual_camera.py`.

**Output:** Decision doc for each deferred PR (pick/skip/wait + rationale).

---

## Dependencies

| This Step | Depends On | Blocks |
|:-:|---|---|
| 2 | 1 (upstream remote) | 3 (validation) |
| 3 | 2 (Batch 1 merged) | 4 (Batch 2) |
| 4 | 3 (V100 validated) | 5 (docs) |
| 5 | 4 (Batch 2 merged) | WS2, WS3 start |
| 6 | 2 (Batch 1 merged) | None (advisory) |

## Success Criteria

- [x] 6 PRs cherry-picked (#1682, #1707, #1667, #1683, #1684, #1685)
- [x] V100 FP32 inference stable (zero NaN in 100 frames)
- [x] SSL bypass removed, model checksums validated
- [x] No regressions in image/video/live modes
- [x] CHANGELOG updated
- [x] Batch 3 decisions documented

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|:-:|:-:|---|
| #1707 doesn't fix V100 NaN | Low | Critical | Manual FP32 model path override; test with both models |
| Batch 2 conflicts with Batch 1 | Low | Low | Apply in documented order; manual resolution |
| #1682 breaks model download | Low | Medium | Test download of each model after cherry-pick |
| Upstream merges break our fork | Low | Low | Not rebasing; cherry-pick only |

## Estimated Effort

| Step | Agent | Hours |
|------|-------|:---:|
| 1. Fetch upstream | mmhs | 0.5 |
| 2. Batch 1 picks | mmhs + Claude | 2 |
| 3. V100 validation | mm | 2 |
| 4. Batch 2 picks | mmhs | 2 |
| 5. Documentation | mm | 1 |
| 6. Batch 3 review | Claude | 1 |
| **Total** | | **8.5h** |

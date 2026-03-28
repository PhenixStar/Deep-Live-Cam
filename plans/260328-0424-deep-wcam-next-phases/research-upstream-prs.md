# Upstream PR Analysis: Deep-Live-Cam Open PRs
**Research Date:** 2026-03-28 04:24 UTC
**Already Cherry-Picked:** #1661, #1657, #1673, #1655, #1668, #1672, #1681, #1676

---

## Summary

Found **20 remaining open PRs**. Most are from `laurigates` (code quality, perf, macOS-focused). **1 NEW PR** since last check (2026-03-28 02:50): **#1707** (FP32 model default).

**Recommended Action Plan:**
- **BATCH 1 (Safe):** #1707, #1667 — cherry-pick immediately (2 files, low risk, high V100 value)
- **BATCH 2 (Needs Review):** #1688, #1685, #1684, #1683, #1682 — check dependencies and conflicts
- **BATCH 3 (Skip):** #1677, #1675, #1674, #1666, #1647, #1620, #1617, #1616, #1602, #1588 — macOS-specific, UI, or old

---

## Detailed Analysis

### BATCH 1: Safe to Cherry-Pick

#### **#1707** — Switch to FP32 model by default, add run script ⭐ **NEW**
- **Created:** 2026-03-27 (1 day old)
- **Author:** rohanrathi99
- **Files:** 2 | **+19, -4**
- **Relevance:** 🔴 **CRITICAL FOR V100**
  - Switches default ONNX model from `inswapper_128_fp16.onnx` → `inswapper_128.onnx` (FP32)
  - Fixes NaN issues on non-Tensor-Core GPUs (V100 falls here)
  - Adds runtime `run.py` wrapper for PATH config
- **Conflict Risk:** None (touches only face_swapper.py, run.py)
- **Value:** Bug fix + stability improvement
- **Review Status:** Sourcery feedback (minor — suggest making precision configurable, avoid string duplication)
- **Action:** **CHERRY-PICK IMMEDIATELY** — this is the fix we need for V100 NaN stability

---

#### **#1667** — Fix macOS memory limit exabytes→gigabytes
- **Created:** 2026-02-22
- **Author:** laurigates
- **Files:** 1 | **+1, -1**
- **Relevance:** 🟢 **V100-Safe**
  - Fixes `limit_resources()` in `modules/core.py`
  - Changes `1024 ** 6` (exabytes) → `1024 ** 3` (gigabytes) on macOS
  - Linux path unaffected; safe for V100/Linux
- **Conflict Risk:** None (single-file, guard clause only affects macOS)
- **Value:** Bug fix (cosmetic for Linux, critical for macOS users)
- **Action:** **CHERRY-PICK** (no downside for Linux, fixes macOS off-by-billion bug)

---

### BATCH 2: Needs Manual Review

#### **#1688** — Migrate to uv + pyproject.toml
- **Created:** 2026-02-24
- **Author:** laurigates
- **Files:** 5 | **+139, -0**
- **Relevance:** 🟡 **Moderate (build system, not runtime)**
  - Replaces pip + requirements.txt with uv + pyproject.toml
  - Adds mise.toml (Python version management), justfile (task runner)
  - Keeps requirements.txt for backward compat
  - Platform-specific deps: onnxruntime-gpu vs onnxruntime-silicon
- **Conflict Risk:** Medium
  - Pillow version pinned to 12.1.1 (likely doesn't exist on PyPI — Sourcery flagged this)
  - onnxruntime-gpu marker will pull GPU deps on Linux (correct for V100)
  - Missing CPU-only fallback in platform markers
- **Value:** Build/tooling improvement (uv is faster, more reliable lockfiles)
- **Review Status:** Sourcery found 2 issues (fake Pillow version, platform markers need tightening)
- **Action:** **DEFER** — wait for author to fix Pillow/marker issues, then cherry-pick

---

#### **#1685** — Refactor: extract pair-building helpers, reduce nesting
- **Created:** 2026-02-23
- **Author:** laurigates
- **Files:** 2 | **+97, -93**
- **Relevance:** 🟢 **Code quality only**
  - Pure refactor in face-swapper (pair-building logic extraction)
  - No behavioral change
- **Conflict Risk:** Low (internal refactor, changes line numbers but not APIs)
- **Value:** Maintainability improvement
- **Action:** **CHERRY-PICK** (after #1707 to avoid rebase churn)

---

#### **#1684** — Refactor: extract process_frames_io helper
- **Created:** 2026-02-23
- **Author:** laurigates
- **Files:** 2 | **+29, -19**
- **Relevance:** 🟢 **Code quality only**
  - Extracts frame I/O loop into helper
  - No behavioral change
- **Conflict Risk:** Low
- **Value:** Maintainability
- **Action:** **CHERRY-PICK** (after #1707)

---

#### **#1683** — Refactor: extract process_frames_io helper
- **Created:** 2026-02-23
- **Author:** laurigates
- **Files:** 1 | **+7, -0**
- **Relevance:** 🟢 **Code quality only**
  - Named constants for magic numbers in globals
  - No behavioral change
- **Conflict Risk:** None
- **Value:** Maintainability
- **Action:** **CHERRY-PICK** (low risk, good practice)

---

#### **#1682** — Fix: remove SSL bypass, log silent errors, add checksum validation
- **Created:** 2026-02-23
- **Author:** laurigates
- **Files:** 2 | **+36, -11**
- **Relevance:** 🔴 **Security fix**
  - Removes SSL bypass (critical security issue)
  - Adds error logging for failed downloads
  - Adds model checksum validation
  - Touches model download logic (face_swapper, face-enhancer)
- **Conflict Risk:** Low (security fix, no behavior changes for valid models)
- **Value:** **HIGH** — security fix, reliability improvement
- **Action:** **CHERRY-PICK** (do this before #1707 to ensure SSL works)

---

### BATCH 3: Skip

#### **#1677** — Perf: add warmup inference call after CoreML model load
- **Relevance:** 🔵 **macOS-only** (CoreML is Apple-specific)
- **Action:** **SKIP**

---

#### **#1675** — Perf: add NumPy Apple Accelerate BLAS detection
- **Relevance:** 🔵 **macOS-only** (Apple Accelerate BLAS)
- **Action:** **SKIP**

---

#### **#1674** — Feat: add runtime det_size switching for Apple Silicon
- **Relevance:** 🔵 **macOS-only** (Apple Silicon optimization)
- **Action:** **SKIP**

---

#### **#1666** — Feat: add built-in virtual camera output
- **Created:** 2026-02-22
- **Author:** laurigates
- **Files:** 5 | **+136, -0**
- **Relevance:** 🟡 **Linux support but complex**
  - Adds pyvirtualcam module for Zoom/Meet/Discord virtual camera
  - Linux: needs v4l2loopback (extra dep not in requirements.txt)
  - macOS/Windows: OBS Virtual Camera integration
  - Optional feature (graceful fallback if pyvirtualcam not installed)
- **Conflict Risk:** Medium (pyvirtualcam not in current requirements.txt, may be unmaintained)
- **Value:** Nice-to-have feature (not core functionality)
- **Action:** **DEFER** — low priority, requires extra system deps

---

#### **#1647** — Bump protobuf from 4.25.1 to 5.29.6 (Dependabot)
- **Relevance:** 🟡 **Dependency bump**
- **Status:** Old (2026-02-05), likely superseded by newer bumps
- **Action:** **SKIP** (let dependabot handle this)

---

#### **#1620** — Submit button visibility with 19+ target faces (Bug fix)
- **Created:** 2026-01-06
- **Author:** imabdullahajmal
- **Relevance:** 🟢 **V100-relevant** but UI-specific
- **Status:** 2+ months old, no recent activity
- **Action:** **DEFER** — check if still reproducible; low priority

---

#### **#1617** — Optimize macOS frame rate
- **Relevance:** 🔵 **macOS-only**
- **Action:** **SKIP**

---

#### **#1616** — Fix NameError: face_outline_points undefined
- **Created:** 2026-01-03
- **Author:** decrypto27
- **Relevance:** 🟢 **Potential V100 bug fix**
- **Status:** 3 months old, appears abandoned
- **Action:** **DEFER** — if this error occurs in our setup, cherry-pick; otherwise skip

---

#### **#1602** — UI fix: crash on live cam on Arch Linux
- **Created:** 2025-12-07
- **Author:** binarytrails
- **Relevance:** 🟢 **Linux-relevant**
- **Status:** Very old (4 months), likely specific to tkinter/Arch
- **Action:** **DEFER** — check if relevant; low priority

---

#### **#1588** — Feat: UI overhaul (theme toggle, tooltips, layout)
- **Created:** 2025-11-21
- **Author:** Pr3zLy
- **Files:** 1 (large)
- **Relevance:** 🔵 **UI-only, not core**
- **Status:** 4 months old, no recent reviews
- **Action:** **SKIP** (scope creep, not critical for V100)

---

## Recommended Cherry-Pick Order

### **Phase 1: Security + Stability (Immediate)**
1. **#1682** — Remove SSL bypass, add checksum validation
2. **#1707** — Switch to FP32 (V100 NaN fix)
3. **#1667** — Fix macOS memory limit

### **Phase 2: Code Quality (After Phase 1)**
4. **#1683** — Named constants for magic numbers
5. **#1684** — Extract process_frames_io helper
6. **#1685** — Extract pair-building helpers

### **Phase 3: Hold + Monitor**
- **#1688** (uv/pyproject.toml) — wait for Pillow/marker fixes
- **#1620, #1616** — defer unless we reproduce the bugs
- **#1602, #1666, #1617, #1677, #1675, #1674** — skip (platform-specific or low-priority)

---

## Unresolved Questions

1. Has #1707 (FP32 fix) been tested on V100 GPUs specifically? (Sourcery suggests it's for non-Tensor-Core GPUs but no explicit V100 test mentioned)
2. Should we wait for #1688 to mature (Pillow version fix) or fork it locally?
3. Does #1620 (19+ faces UI bug) affect our workflow?
4. Should #1666 (virtual camera) be kept on backlog for future releases?

## Code Review Summary

### Scope
- Files reviewed: 7
  - `modules/paths.py` (new)
  - `modules/processors/frame/face_swapper.py` (modified)
  - `modules/processors/frame/face_enhancer.py` (modified)
  - `modules/processors/frame/face_enhancer_gpen256.py` (modified)
  - `modules/processors/frame/face_enhancer_gpen512.py` (modified)
  - `start.sh` (new)
  - `setup.sh` (new)
- Focus: centralization of model directory path, shell script correctness, import validity, security

### Overall Assessment

The changes are clean, well-scoped, and accomplish the goal of centralizing the models directory into a single source of truth (`modules/paths.py`) with env-var override capability. The import pattern works correctly, shell scripts are functional, and no security issues were found.

---

### Critical Issues

None.

---

### High Priority

**1. `insightface` model directory is NOT governed by `DEEP_LIVE_CAM_MODELS_DIR`**

`modules/face_analyser.py` (line 28) and `modules/processors/frame/face_swapper.py` (line 111) use `insightface.app.FaceAnalysis(name='buffalo_l', ...)` and `insightface.model_zoo.get_model(model_path, ...)`. The FaceAnalysis class downloads its detection/recognition models to `~/.insightface/models/buffalo_l/` by default, completely bypassing the centralized `MODELS_DIR`.

This means the env-var override only controls where the swap/enhancer ONNX models live, not the InsightFace analysis models. If the intent is to make all model storage relocatable (e.g., for Docker, shared NFS, or symlinked model caches), this gap means InsightFace models will still land in the user home directory.

Impact: Partial coverage of the centralization goal. Users who set `DEEP_LIVE_CAM_MODELS_DIR` expecting all models to be in one place will have InsightFace models elsewhere.

Recommendation: Pass `root=MODELS_DIR` to `insightface.app.FaceAnalysis()` to bring it under the same umbrella, or document this limitation explicitly.

---

### Medium Priority

**2. Import placement style inconsistency**

In all four processor files, the import from `modules.paths` is placed after constant/global definitions rather than with the other imports at the top. For example in `face_enhancer.py`:

```python
FACE_ENHANCER = None        # line 22
THREAD_SEMAPHORE = ...      # line 23
THREAD_LOCK = ...           # line 24
NAME = ...                  # line 25
                            # line 26 blank
from modules.paths import MODELS_DIR as models_dir  # line 27
```

This works fine at runtime, but PEP 8 and most linters expect all imports grouped at the top of the file. In `face_swapper.py` it appears on line 41 after ~15 lines of global constants.

Impact: Low functional risk. Linter noise and readability.

Recommendation: Move the import to the top import block in each file.

**3. `setup.sh` hardcodes `mkdir -p models` (line 29)**

The setup script creates `models/` relative to the project root (`mkdir -p models`) and downloads into it with `huggingface-cli download ... --local-dir models`. This does not respect `DEEP_LIVE_CAM_MODELS_DIR`. If a user sets the env var before running setup, the downloaded models and the runtime models directory will be in different locations.

Impact: Setup/runtime mismatch when env var is used.

Recommendation: Replace the hardcoded path in setup.sh:
```bash
MODELS_DIR="${DEEP_LIVE_CAM_MODELS_DIR:-$(pwd)/models}"
mkdir -p "$MODELS_DIR"
huggingface-cli download hacksider/deep-live-cam \
  --local-dir "$MODELS_DIR" --local-dir-use-symlinks False
```

**4. `setup.sh` line 18: `grep -v pygrabber` writes to `/tmp/req-linux.txt`**

```bash
grep -v pygrabber requirements.txt > /tmp/req-linux.txt
pip install -r /tmp/req-linux.txt
```

Writing to a predictable path in `/tmp` has a minor symlink-attack surface (CWE-377). On multi-user systems another user could pre-create `/tmp/req-linux.txt` as a symlink. This is low risk for a developer setup script but worth noting.

Recommendation: Use `mktemp` instead:
```bash
REQFILE=$(mktemp /tmp/req-linux.XXXXXX)
grep -v pygrabber requirements.txt > "$REQFILE"
pip install -r "$REQFILE"
rm -f "$REQFILE"
```

---

### Low Priority

**5. `start.sh` does not check `.venv` existence before sourcing**

If `start.sh` is run before `setup.sh`, it will fail with a confusing error when `source .venv/bin/activate` fails. A guard would improve UX:

```bash
[ -f .venv/bin/activate ] || { echo "Run ./setup.sh first"; exit 1; }
```

**6. `setup.sh` symlink logic (line 32)**

```bash
[ -L models/inswapper_128.onnx ] || ln -sf inswapper_128_fp16.onnx models/inswapper_128.onnx
```

The `-L` test checks if the symlink exists, but if a regular file named `inswapper_128.onnx` already exists (e.g., someone downloaded the full-precision model), `ln -sf` will silently overwrite it. This is probably intended behavior but worth noting. Also, `ln -sf` with a relative target means the link target is relative to the link location, which is correct here since both are in `models/`.

**7. Alias naming convention**

All four files import as `from modules.paths import MODELS_DIR as models_dir`. The alias uses `snake_case` for what is a module-level constant. This works fine but is slightly misleading -- the value is immutable after import. Minor style point; consistency across the files is good.

---

### Edge Cases Found by Scout

1. **`insightface` model directory divergence** (see High Priority #1 above) -- the most significant finding. The centralization covers the project's own ONNX models but not the third-party InsightFace model cache.

2. **`modules/__init__.py` has a bug** -- line 15 has `ext` reformatted redundantly:
   ```python
   result, encoded_img = cv2.imencode(ext, img, params if params else [])
   result, encoded_img = cv2.imencode(f".{ext}", img, params if params is not None else [])
   ```
   The first `imencode` call result is immediately overwritten by the second. The second prepends a dot to `ext` which already has one from `os.path.splitext()`. This is a pre-existing bug, not introduced by this change, but it will cause double-dot extensions (e.g., `..png`). This is unrelated to the models-dir work but was discovered during scouting.

3. **Import chain validated** -- `modules/__init__.py`, `modules/processors/__init__.py`, and `modules/processors/frame/__init__.py` all exist, so the import `from modules.paths import MODELS_DIR` works correctly from any depth in the package hierarchy.

4. **`ROOT_DIR` calculation is correct** -- `os.path.dirname(os.path.dirname(os.path.abspath(__file__)))` from `modules/paths.py` correctly resolves to the project root since `paths.py` is one level down in `modules/`.

---

### Positive Observations

- Single source of truth for models directory is the right architectural move
- Env var name `DEEP_LIVE_CAM_MODELS_DIR` is well-namespaced to avoid collisions
- The fallback to `os.path.join(ROOT_DIR, "models")` preserves backward compatibility
- `start.sh` correctly uses `${VAR:-default}` syntax for safe env-var defaults
- `setup.sh` uses `set -e` for fail-fast behavior
- All four processor files were consistently updated
- No secrets or credentials in any of the changes
- Shell scripts have correct Unix line endings and executable permissions

---

### Recommended Actions

1. **Consider** passing `root=MODELS_DIR` to `insightface.app.FaceAnalysis()` to fully centralize model storage
2. **Fix** `setup.sh` to use `DEEP_LIVE_CAM_MODELS_DIR` env var for consistency with runtime
3. **Move** `from modules.paths import MODELS_DIR as models_dir` to the top-of-file import block in each processor file
4. **Add** a `.venv` existence guard to `start.sh`
5. (Optional) Replace `/tmp/req-linux.txt` with `mktemp` in `setup.sh`

---

### Unresolved Questions

1. Is the intent to make InsightFace model storage relocatable as well, or is that out of scope?
2. Should `setup.sh` be idempotent with respect to the cuDNN `LD_LIBRARY_PATH` append (currently it uses `grep -q` to guard, which is good, but the path is baked at setup time -- if the venv is moved, the path in `activate` becomes stale)?

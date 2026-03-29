# Deferred Issues & Future Work

## Rebranding: "Deep Live Cam" → "Deep Forge" / "DeFo"

- [x] Rename `productName` in `app/src-tauri/tauri.conf.json`
- [x] Update `identifier` from `com.phenixstar.deep-live-cam` to `com.phenixstar.deep-forge`
- [x] Update `app/package.json` name
- [x] Rename Cargo package names (`deep-live-cam-app` → `deep-forge-app`)
- [x] Update sidecar binary name references (`deep-forge-server`)
- [x] Update `resolve_server_exe()` patterns
- [x] Update all UI text ("Deep Live Cam" → "Deep Forge")
- [x] Update window title
- [x] Update install directory name
- [x] Update README, CONTRIBUTING

## Source Face Gallery / Profile Catalog

- [ ] Save uploaded source faces with scores to a local gallery
- [ ] Gallery panel with thumbnails + detection scores
- [ ] Quick-select from gallery (no re-upload needed)
- [ ] Name/tag profiles for fast identification
- [ ] Pre-compute and cache embeddings per profile

## PowerShell Console Window

- [x] Fixed with CREATE_NO_WINDOW flag

## Model Loading Issues

- [ ] GFPGAN (gfpgan-1024.onnx) fails protobuf parsing with ORT 1.24 — may need older model version or different model file
- [ ] GPEN-512 same issue — investigate ORT compatibility
- [ ] GPEN-256 works fine

## Camera Improvements

- [ ] Camera takes 30s to open on Windows MSMF — investigate DirectShow backend as alternative
- [ ] Add camera status indicator in UI (opening/ready/failed)
- [ ] Hot-plug detection for USB cameras

## Performance

- [ ] Currently ~6 FPS with DirectML — investigate FP16 model conversion for 2x throughput
- [ ] Cache source face embedding (don't re-detect source every frame)
- [ ] Consider NPU offloading for detection (Phase 2 of GPU plan)

## CI/CD

- [ ] .github/workflows/ci.yml changes need PAT with `workflow` scope to push

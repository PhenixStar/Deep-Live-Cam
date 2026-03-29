# Phase 3: Model Management UI

**Priority:** P1
**Effort:** 8h
**Status:** Pending
**Depends on:** Phase 2 (uses metrics panel pattern)

## Overview

Add model status display, download progress, and configuration to the UI. Currently users must manually place ONNX files — the app should detect missing models and offer to download them.

## Related Code Files

### Modify
- `core/rust-engine/dlc-server/src/router.rs` — Add `GET /models/status` endpoint
- `core/rust-engine/dlc-server/src/state.rs` — Add models_dir to health response
- `app/src-tauri/src/main.rs` — Add Tauri download commands

### Create
- `app/src/components/model-manager.tsx` — Model status cards + download buttons
- `app/src-tauri/src/model_download.rs` — Tauri commands for model downloads with progress events

## Implementation Steps

### Step 1: Add GET /models/status endpoint

Returns per-model status:
```json
{
  "models": [
    { "name": "SCRFD (Face Detection)", "file": "buffalo_l/buffalo_l/det_10g.onnx", "loaded": true, "file_exists": true, "size_mb": 16.7, "required": true },
    { "name": "ArcFace (Recognition)", "file": "buffalo_l/buffalo_l/w600k_r50.onnx", "loaded": true, "file_exists": true, "size_mb": 166.8, "required": true },
    { "name": "inswapper (Face Swap)", "file": "inswapper_128.onnx", "loaded": true, "file_exists": true, "size_mb": 265.0, "required": true },
    { "name": "GFPGAN (Enhancer)", "file": "gfpgan-1024.onnx", "loaded": false, "file_exists": false, "size_mb": null, "required": false },
    { "name": "GPEN-256", "file": "GPEN-BFR-256.onnx", "loaded": false, "file_exists": false, "size_mb": null, "required": false },
    { "name": "GPEN-512", "file": "GPEN-BFR-512.onnx", "loaded": false, "file_exists": false, "size_mb": null, "required": false }
  ]
}
```

### Step 2: Model download via Tauri commands

Use `reqwest` in the Tauri shell (not dlc-server) with progress events:
```rust
#[tauri::command]
async fn download_model(app: AppHandle, model_name: String, url: String, dest: String) -> Result<(), String> {
    let response = reqwest::get(&url).await.map_err(|e| e.to_string())?;
    let total = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut file = tokio::fs::File::create(&dest).await.map_err(|e| e.to_string())?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;
        app.emit("model_download_progress", json!({
            "model": model_name, "downloaded": downloaded, "total": total
        })).ok();
    }
    Ok(())
}
```

### Step 3: Model manager UI component

Shows cards per model: name, status (loaded/missing/downloading), size, download button with progress bar.

### Step 4: Startup model check

On app mount, fetch `/models/status`. If any required model is missing, show a prominent banner: "Missing models — click to download".

## Todo

- [ ] Add `GET /models/status` endpoint
- [ ] Create Tauri download command with progress events
- [ ] Create model-manager React component
- [ ] Add startup model check with banner
- [ ] Add model download URLs as constants
- [ ] Add models_dir to settings response

## Success Criteria

- Missing models clearly indicated in UI
- One-click download with progress bar
- Model status refreshes after download completes
- Server reloads models after download (or shows "restart required")

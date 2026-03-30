//! Axum router construction and all HTTP/WebSocket handlers.
//!
//! Extracted into a separate module so `lib.rs` can re-export `build_router`
//! for use by integration tests without pulling in `main`.

use axum::{
    Router,
    body::Bytes,
    extract::{Path, Json, State, ws::{WebSocket, WebSocketUpgrade}},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post, delete},
};
use serde::Deserialize;
use std::sync::{Arc, Mutex, RwLock};
use tower_http::cors::{CorsLayer, Any};

use crate::state::{AppState, InputMode};
use dlc_core::{detect::FaceDetector, swap::FaceSwapper, enhance::FaceEnhancer, GpuProvider, Frame};

// ---------------------------------------------------------------------------
// Metrics types
// ---------------------------------------------------------------------------

#[derive(Clone, serde::Serialize)]
pub struct FaceRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub score: f32,
}

#[derive(Clone, serde::Serialize)]
pub struct FrameMetrics {
    pub detect_ms: f64,
    pub swap_ms: f64,
    pub total_ms: f64,
    pub face_count: usize,
    pub faces: Vec<FaceRect>,
    /// Bounding box of the swapped face region (for red overlay).
    pub swap_bbox: Option<FaceRect>,
}

impl Default for FrameMetrics {
    fn default() -> Self {
        Self {
            detect_ms: 0.0,
            swap_ms: 0.0,
            total_ms: 0.0,
            face_count: 0,
            swap_bbox: None,
            faces: vec![],
        }
    }
}

// ---------------------------------------------------------------------------
// Model container — uses std::sync::Mutex so models can be used from
// both async handlers and spawn_blocking threads.
// ---------------------------------------------------------------------------

pub struct Models {
    pub detector: Mutex<Option<FaceDetector>>,
    pub swapper:  Mutex<Option<FaceSwapper>>,
    pub enhancer_gfpgan:  Mutex<Option<dlc_core::enhance::FaceEnhancer>>,
    pub enhancer_gpen256: Mutex<Option<dlc_core::enhance::FaceEnhancer>>,
    pub enhancer_gpen512: Mutex<Option<dlc_core::enhance::FaceEnhancer>>,
    /// Global DirectML inference lock. AMD GPUs crash when multiple DML
    /// sessions run concurrently (upstream hacksider/Deep-Live-Cam PR #1710).
    /// All inference calls must hold this lock to serialize GPU access.
    pub dml_lock: Mutex<()>,
}

// ---------------------------------------------------------------------------
// Combined server state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ServerState {
    pub app:            Arc<RwLock<AppState>>,
    pub models:         Arc<Models>,
    pub camera:         Arc<Mutex<Option<dlc_capture::CameraCapture>>>,
    pub metrics_tx:     tokio::sync::broadcast::Sender<String>,
    pub gpu_provider:   String,
    pub remote_mode:    bool,
    pub bind_address:   String,
    pub api_token:      Option<String>,
    /// Number of currently connected WS video clients.
    pub connected_clients: Arc<std::sync::atomic::AtomicU32>,
    /// OpenCV VideoWriter for recording (None when not recording).
    /// Uses an Arc<Mutex<Option<...>>> so the producer thread can write frames.
    #[cfg(feature = "opencv")]
    pub recording_writer: Arc<Mutex<Option<opencv::videoio::VideoWriter>>>,
}

impl axum::extract::FromRef<ServerState> for Arc<RwLock<AppState>> {
    fn from_ref(s: &ServerState) -> Self {
        s.app.clone()
    }
}

// ---------------------------------------------------------------------------
// Public router factory
// ---------------------------------------------------------------------------

/// Build the application router with the given state.
///
/// Tests construct `ServerState` directly with `models: None` to avoid
/// loading ONNX files from disk.
pub fn build_router(server_state: ServerState, remote: bool) -> Router {
    let cors = if remote {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        // Tauri v2 uses http://tauri.localhost on Windows, tauri://localhost on macOS/Linux
        CorsLayer::new()
            .allow_origin([
                "tauri://localhost".parse().unwrap(),
                "http://tauri.localhost".parse().unwrap(),
                "https://tauri.localhost".parse().unwrap(),
                "http://localhost:1420".parse().unwrap(),
                "http://localhost:8008".parse().unwrap(),
            ])
            .allow_methods(Any)
            .allow_headers(Any)
    };

    let mut router = Router::new()
        .route("/health",          get(health))
        .route("/source",          post(upload_source))
        .route("/swap/image",      post(swap_image))
        .route("/cameras",         get(list_cameras))
        .route("/cameras/refresh", post(refresh_cameras))
        .route("/camera/{index}",  post(set_camera))
        .route("/settings",        get(get_settings).post(update_settings))
        .route("/models/status",   get(models_status))
        .route("/models/reload",   post(reload_models))
        .route("/ws/video",        get(ws_video))
        .route("/ws/metrics",      get(ws_metrics))
        .route("/profiles",        get(crate::profiles::list_profiles).post(crate::profiles::create_profile))
        .route("/profiles/{id}",   get(crate::profiles::get_profile).put(crate::profiles::update_profile).delete(crate::profiles::delete_profile))
        .route("/profiles/{id}/photos",      post(crate::profiles::add_photo))
        .route("/profiles/{id}/photos/{idx}", delete(crate::profiles::delete_photo))
        .route("/profiles/{id}/activate",    post(crate::profiles::activate_profile))
        .route("/input/video",      post(input_set_video))
        .route("/input/camera",     post(input_set_camera))
        .route("/input/status",     get(input_status))
        .route("/camera/status",    get(camera_status))
        .route("/recording/start",  post(recording_start))
        .route("/recording/stop",   post(recording_stop))
        .route("/providers",         get(list_providers))
        .route("/providers/switch",  post(switch_provider))
        .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(cors);

    // In remote mode, validate token on all requests except /health.
    // Accepts both: Authorization: Bearer <token> OR X-Deep-Forge-Token: <token>
    if remote {
        if let Some(ref token) = server_state.api_token {
            let expected_bearer = format!("Bearer {token}");
            let expected_raw = token.clone();
            router = router.layer(axum::middleware::from_fn(
                move |req: axum::http::Request<axum::body::Body>, next: axum::middleware::Next| {
                    let expected_bearer = expected_bearer.clone();
                    let expected_raw = expected_raw.clone();
                    async move {
                        if req.uri().path() == "/health" {
                            return next.run(req).await;
                        }
                        // Check Authorization: Bearer <token>
                        let bearer_ok = req.headers()
                            .get(axum::http::header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok())
                            .map(|v| v == expected_bearer)
                            .unwrap_or(false);
                        // Check X-Deep-Forge-Token: <token>
                        let custom_ok = req.headers()
                            .get("X-Deep-Forge-Token")
                            .and_then(|v| v.to_str().ok())
                            .map(|v| v == expected_raw)
                            .unwrap_or(false);
                        if bearer_ok || custom_ok {
                            next.run(req).await
                        } else {
                            (StatusCode::UNAUTHORIZED,
                             Json(serde_json::json!({"error": "invalid or missing token (use Authorization: Bearer or X-Deep-Forge-Token header)"})))
                                .into_response()
                        }
                    }
                },
            ));
        }
    }

    router.with_state(server_state)
}

/// Build a `ServerState` with no models loaded (safe for unit/integration tests).
pub fn test_state() -> ServerState {
    let (metrics_tx, _) = tokio::sync::broadcast::channel(64);
    ServerState {
        app:          Arc::new(RwLock::new(AppState::default())),
        camera:       Arc::new(Mutex::new(None)),
        models:       Arc::new(Models {
            detector: Mutex::new(None),
            swapper:  Mutex::new(None),
            enhancer_gfpgan:  Mutex::new(None),
            enhancer_gpen256: Mutex::new(None),
            enhancer_gpen512: Mutex::new(None),
            dml_lock: Mutex::new(()),
        }),
        metrics_tx,
        gpu_provider: "Auto".to_string(),
        remote_mode:  false,
        bind_address: "127.0.0.1:8008".to_string(),
        #[cfg(feature = "opencv")]
        recording_writer: Arc::new(Mutex::new(None)),
        api_token:    None,
        connected_clients: Arc::new(std::sync::atomic::AtomicU32::new(0)),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn health(State(server_state): State<ServerState>) -> impl IntoResponse {
    let models = &server_state.models;
    let detector_ok = models.detector.lock().map(|g| g.is_some()).unwrap_or(false);
    let swapper_ok  = models.swapper.lock().map(|g| g.is_some()).unwrap_or(false);
    let gfpgan_ok   = models.enhancer_gfpgan.lock().map(|g| g.is_some()).unwrap_or(false);
    let gpen256_ok  = models.enhancer_gpen256.lock().map(|g| g.is_some()).unwrap_or(false);
    let gpen512_ok  = models.enhancer_gpen512.lock().map(|g| g.is_some()).unwrap_or(false);

    Json(serde_json::json!({
        "status": "ok",
        "backend": "rust",
        "gpu_provider": server_state.gpu_provider,
        "models": {
            "detector": detector_ok,
            "swapper":  swapper_ok,
            "gfpgan":   gfpgan_ok,
            "gpen256":  gpen256_ok,
            "gpen512":  gpen512_ok,
        },
        "remote_mode": server_state.remote_mode,
        "bind_address": server_state.bind_address,
        "connected_clients": server_state.connected_clients.load(std::sync::atomic::Ordering::Relaxed),
    }))
}

async fn models_status(
    State(server_state): State<ServerState>,
) -> impl IntoResponse {
    let app = server_state.app.read().unwrap();
    let mut statuses = crate::model_manifest::check_models(&app.models_dir);

    // Cross-reference with actually loaded models (from Models struct).
    let models = &server_state.models;
    let loaded_map: std::collections::HashMap<&str, bool> = [
        ("buffalo_l/buffalo_l/det_10g.onnx", models.detector.lock().map(|g| g.is_some()).unwrap_or(false)),
        ("buffalo_l/buffalo_l/w600k_r50.onnx", models.swapper.lock().map(|g| g.is_some()).unwrap_or(false)),
        ("inswapper_128.onnx", models.swapper.lock().map(|g| g.is_some()).unwrap_or(false)),
        ("inswapper_128_fp16.onnx", models.swapper.lock().map(|g| g.is_some()).unwrap_or(false)),
        ("gfpgan-1024.onnx", models.enhancer_gfpgan.lock().map(|g| g.is_some()).unwrap_or(false)),
        ("GPEN-BFR-256.onnx", models.enhancer_gpen256.lock().map(|g| g.is_some()).unwrap_or(false)),
        ("GPEN-BFR-512.onnx", models.enhancer_gpen512.lock().map(|g| g.is_some()).unwrap_or(false)),
    ].into();

    // Enrich statuses with `loaded` field.
    let enriched: Vec<serde_json::Value> = statuses.iter().map(|s| {
        let loaded = loaded_map.get(s.info.path).copied().unwrap_or(false);
        let mut v = serde_json::to_value(s).unwrap_or_default();
        v.as_object_mut().map(|o| o.insert("loaded".into(), loaded.into()));
        v
    }).collect();

    Json(serde_json::json!({
        "models": enriched,
        "gpu_provider": server_state.gpu_provider,
    }))
}

async fn reload_models(State(state): State<ServerState>) -> impl IntoResponse {
    let (models_dir, selected_provider_str) = {
        let app = state.app.read().unwrap();
        (app.models_dir.clone(), app.selected_provider.clone())
    };
    let provider = provider_from_str(&selected_provider_str);
    let mut results = serde_json::Map::new();

    // Detector
    let det_path = models_dir.join("buffalo_l/buffalo_l/det_10g.onnx");
    match FaceDetector::new(&det_path, &provider) {
        Ok(d) => {
            *state.models.detector.lock().unwrap() = Some(d);
            results.insert("detector".into(), serde_json::Value::String("loaded".into()));
        }
        Err(e) => {
            results.insert("detector".into(), serde_json::Value::String(format!("failed: {e}")));
        }
    }

    // Swapper
    match dlc_core::swap::FaceSwapper::new(&models_dir, &provider) {
        Ok(s) => {
            *state.models.swapper.lock().unwrap() = Some(s);
            results.insert("swapper".into(), serde_json::Value::String("loaded".into()));
        }
        Err(e) => {
            results.insert("swapper".into(), serde_json::Value::String(format!("failed: {e}")));
        }
    }

    // GFPGAN enhancer
    let gfpgan_path = models_dir.join("gfpgan-1024.onnx");
    match FaceEnhancer::new(&gfpgan_path, 1024, &provider) {
        Ok(e) => {
            *state.models.enhancer_gfpgan.lock().unwrap() = Some(e);
            results.insert("gfpgan".into(), serde_json::Value::String("loaded".into()));
        }
        Err(e) => {
            results.insert("gfpgan".into(), serde_json::Value::String(format!("failed: {e}")));
        }
    }

    // GPEN-256 enhancer
    let gpen256_path = models_dir.join("GPEN-BFR-256.onnx");
    match FaceEnhancer::new(&gpen256_path, 256, &provider) {
        Ok(e) => {
            *state.models.enhancer_gpen256.lock().unwrap() = Some(e);
            results.insert("gpen256".into(), serde_json::Value::String("loaded".into()));
        }
        Err(e) => {
            results.insert("gpen256".into(), serde_json::Value::String(format!("failed: {e}")));
        }
    }

    // GPEN-512 enhancer
    let gpen512_path = models_dir.join("GPEN-BFR-512.onnx");
    match FaceEnhancer::new(&gpen512_path, 512, &provider) {
        Ok(e) => {
            *state.models.enhancer_gpen512.lock().unwrap() = Some(e);
            results.insert("gpen512".into(), serde_json::Value::String("loaded".into()));
        }
        Err(e) => {
            results.insert("gpen512".into(), serde_json::Value::String(format!("failed: {e}")));
        }
    }

    tracing::info!("[RELOAD] Models reloaded: {:?}", results);
    Json(serde_json::json!({"status": "ok", "models": results}))
}

async fn upload_source(
    State(server_state): State<ServerState>,
    mut multipart: axum::extract::Multipart,
) -> impl IntoResponse {
    let state = server_state.app.clone();
    let field = match multipart.next_field().await {
        Ok(Some(f)) => f,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "no file field in multipart body"})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("multipart error: {e}");
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("multipart error: {e}")})),
            )
                .into_response();
        }
    };

    let bytes = match field.bytes().await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("failed to read field bytes: {e}")})),
            )
                .into_response();
        }
    };

    if let Err(e) = image::load_from_memory(&bytes) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": format!("invalid image: {e}")})),
        )
            .into_response();
    }

    tracing::info!("source image received: {} bytes", bytes.len());

    // Pre-compute face detection + embedding (biggest FPS win — saves ~35ms/frame).
    let mut detected_face = None;
    let mut cached_embedding = None;
    let mut score = None;

    if let Ok(mut models) = server_state.models.detector.lock() {
        if let Some(ref mut detector) = *models {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let rgb = img.to_rgb8();
                let (w, h) = (rgb.width() as usize, rgb.height() as usize);
                // Convert to BGR Array3 for detection.
                let mut bgr = ndarray::Array3::<u8>::zeros((h, w, 3));
                for y in 0..h {
                    for x in 0..w {
                        let p = rgb.get_pixel(x as u32, y as u32);
                        bgr[[y, x, 0]] = p[2]; // B
                        bgr[[y, x, 1]] = p[1]; // G
                        bgr[[y, x, 2]] = p[0]; // R
                    }
                }
                if let Ok(faces) = detector.detect(&bgr, 0.3) {
                    if let Some(face) = faces.into_iter().next() {
                        score = Some(face.score);
                        // Try to get embedding too.
                        drop(models); // release detector lock
                        if let Ok(mut swapper_guard) = server_state.models.swapper.lock() {
                            if let Some(ref mut swapper) = *swapper_guard {
                                if let Ok(emb) = swapper.get_embedding(&bgr, &face) {
                                    cached_embedding = Some(emb.clone());
                                    let mut face_with_emb = face.clone();
                                    face_with_emb.embedding = Some(emb);
                                    detected_face = Some(face_with_emb);
                                    tracing::info!("Source embedding cached (skip re-detect per frame)");
                                }
                            }
                        }
                        if detected_face.is_none() {
                            detected_face = Some(face);
                        }
                    }
                }
            }
        }
    }

    let mut s = state.write().unwrap();
    s.source_image_bytes = Some(bytes.to_vec());
    s.source_face = detected_face;
    s.cached_source_embedding = cached_embedding;

    Json(serde_json::json!({
        "status": "ok",
        "bytes": bytes.len(),
        "score": score,
    })).into_response()
}

// ---------------------------------------------------------------------------
// Input mode endpoints — POST /input/video, POST /input/camera, GET /input/status
// ---------------------------------------------------------------------------

/// POST /input/video
/// Accepts a multipart upload of a video file (.mp4/.avi/.webm).
/// Saves to a temp file and sets input_mode = VideoFile.
/// Live-stream face swap from a video file requires opencv (not compiled in);
/// the WS producer will return a 501 placeholder frame for VideoFile mode.
async fn input_set_video(
    State(state): State<Arc<RwLock<AppState>>>,
    mut multipart: axum::extract::Multipart,
) -> impl IntoResponse {
    let field = match multipart.next_field().await {
        Ok(Some(f)) => f,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "no file field in multipart body"})),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("multipart error: {e}")})),
            )
                .into_response();
        }
    };

    // Capture original filename before consuming field.
    let original_name = field
        .file_name()
        .unwrap_or("video")
        .to_string();

    let bytes = match field.bytes().await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("failed to read field bytes: {e}")})),
            )
                .into_response();
        }
    };

    if bytes.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "empty file"})),
        )
            .into_response();
    }

    // Determine extension from original filename; default to .mp4.
    let ext = std::path::Path::new(&original_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4");

    // Write to a temp file in the system temp dir.
    let tmp_path = std::env::temp_dir().join(format!("deep_forge_input.{ext}"));
    if let Err(e) = std::fs::write(&tmp_path, &bytes) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("failed to save video: {e}")})),
        )
            .into_response();
    }

    tracing::info!(
        "video input uploaded: {} ({} bytes) → {:?}",
        original_name, bytes.len(), tmp_path
    );

    {
        let mut s = state.write().unwrap();
        s.input_mode = InputMode::VideoFile;
        s.video_path = Some(tmp_path.clone());
    }

    Json(serde_json::json!({
        "status": "ok",
        "filename": original_name,
        "bytes": bytes.len(),
        "note": "Live WS streaming from video requires the opencv feature; frames will show a placeholder until supported."
    }))
    .into_response()
}

/// POST /input/camera
/// Switches back to camera input mode.
async fn input_set_camera(
    State(state): State<Arc<RwLock<AppState>>>,
) -> impl IntoResponse {
    {
        let mut s = state.write().unwrap();
        s.input_mode = InputMode::Camera;
        s.video_path = None;
    }
    Json(serde_json::json!({"status": "ok", "input_mode": "camera"}))
}

/// GET /input/status
/// Returns the current input mode and, if VideoFile, the filename.
async fn input_status(
    State(state): State<Arc<RwLock<AppState>>>,
) -> impl IntoResponse {
    let s = state.read().unwrap();
    match s.input_mode {
        InputMode::Camera => Json(serde_json::json!({
            "input_mode": "camera"
        })),
        InputMode::VideoFile => {
            let filename = s
                .video_path
                .as_ref()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            Json(serde_json::json!({
                "input_mode": "video_file",
                "filename": filename
            }))
        }
    }
}

// ---------------------------------------------------------------------------
// GET /camera/status
// ---------------------------------------------------------------------------

async fn camera_status(State(server_state): State<ServerState>) -> impl IntoResponse {
    let has_camera = server_state.camera.lock().unwrap().is_some();
    Json(serde_json::json!({
        "status": if has_camera { "ready" } else { "opening" },
        "available": has_camera,
    }))
}

// ---------------------------------------------------------------------------
// POST /recording/start  |  POST /recording/stop
// ---------------------------------------------------------------------------

async fn recording_start(State(server_state): State<ServerState>) -> impl IntoResponse {
    #[cfg(feature = "opencv")]
    {
        use opencv::videoio::VideoWriter;

        let mut app = server_state.app.write().unwrap();
        if app.recording {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "recording already active"})),
            ).into_response();
        }

        // Build output path: recordings/<timestamp>.mp4 next to models_dir.
        let out_dir = app.models_dir.parent().unwrap_or(&app.models_dir).join("recordings");
        if let Err(e) = std::fs::create_dir_all(&out_dir) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("create recordings dir: {e}")})),
            ).into_response();
        }
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let out_path = out_dir.join(format!("recording_{ts}.mp4"));

        let (width, height) = app.resolution;
        let fourcc = VideoWriter::fourcc('m' as i8, 'p' as i8, '4' as i8, 'v' as i8)
            .unwrap_or(-1);
        match VideoWriter::new(
            &out_path.to_string_lossy(),
            fourcc,
            30.0,
            opencv::core::Size::new(width as i32, height as i32),
            true,
        ) {
            Ok(writer) => {
                *server_state.recording_writer.lock().unwrap() = Some(writer);
                app.recording = true;
                app.recording_path = Some(out_path.clone());
                Json(serde_json::json!({
                    "status": "recording",
                    "path": out_path.to_string_lossy(),
                })).into_response()
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("VideoWriter::new failed: {e}")})),
            ).into_response(),
        }
    }

    #[cfg(not(feature = "opencv"))]
    {
        let _ = server_state;
        (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({"error": "recording requires the opencv feature"})),
        ).into_response()
    }
}

async fn recording_stop(State(server_state): State<ServerState>) -> impl IntoResponse {
    #[cfg(feature = "opencv")]
    {
        let mut app = server_state.app.write().unwrap();
        if !app.recording {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "no active recording"})),
            ).into_response();
        }

        // Drop the VideoWriter — this flushes + closes the file.
        *server_state.recording_writer.lock().unwrap() = None;
        app.recording = false;
        let path = app.recording_path.clone();
        Json(serde_json::json!({
            "status": "stopped",
            "path": path.as_ref().map(|p| p.to_string_lossy().into_owned()),
        })).into_response()
    }

    #[cfg(not(feature = "opencv"))]
    {
        let _ = server_state;
        (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({"error": "recording requires the opencv feature"})),
        ).into_response()
    }
}

// ---------------------------------------------------------------------------
// Image helpers used by swap_image
// ---------------------------------------------------------------------------

pub fn decode_to_bgr_frame(bytes: &[u8]) -> anyhow::Result<Frame> {
    let img = image::load_from_memory(bytes)?.to_rgb8();
    let (w, h) = img.dimensions();
    let mut frame = ndarray::Array3::<u8>::zeros((h as usize, w as usize, 3));
    for (x, y, px) in img.enumerate_pixels() {
        frame[[y as usize, x as usize, 0]] = px[2]; // B
        frame[[y as usize, x as usize, 1]] = px[1]; // G
        frame[[y as usize, x as usize, 2]] = px[0]; // R
    }
    Ok(frame)
}

fn encode_bgr_frame_to_jpeg(frame: &Frame) -> anyhow::Result<Vec<u8>> {
    use image::{ImageBuffer, Rgb, ImageEncoder};
    use image::codecs::jpeg::JpegEncoder;

    let (h, w, _) = frame.dim();
    let mut rgb_buf: Vec<u8> = Vec::with_capacity(h * w * 3);
    for y in 0..h {
        for x in 0..w {
            rgb_buf.push(frame[[y, x, 2]]); // R
            rgb_buf.push(frame[[y, x, 1]]); // G
            rgb_buf.push(frame[[y, x, 0]]); // B
        }
    }
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_raw(w as u32, h as u32, rgb_buf)
            .ok_or_else(|| anyhow::anyhow!("failed to construct RgbImage from frame"))?;

    let mut out: Vec<u8> = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut out, 85);
    encoder.write_image(
        img.as_raw(),
        w as u32,
        h as u32,
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// POST /swap/image
// ---------------------------------------------------------------------------

async fn swap_image(
    State(server_state): State<ServerState>,
    mut multipart: axum::extract::Multipart,
) -> Response {
    let mut source_bytes: Option<Vec<u8>> = None;
    let mut target_bytes: Option<Vec<u8>> = None;

    loop {
        match multipart.next_field().await {
            Ok(None) => break,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": format!("multipart read error: {e}")})),
                )
                    .into_response();
            }
            Ok(Some(field)) => {
                let name = field.name().unwrap_or("").to_string();
                match field.bytes().await {
                    Err(e) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({"error": format!("field read error: {e}")})),
                        )
                            .into_response();
                    }
                    Ok(b) => match name.as_str() {
                        "source" => source_bytes = Some(b.to_vec()),
                        "target" => target_bytes = Some(b.to_vec()),
                        _ => {}
                    },
                }
            }
        }
    }

    let source_bytes = match source_bytes {
        Some(b) => b,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "missing multipart field: source"})),
            )
                .into_response();
        }
    };
    let target_bytes = match target_bytes {
        Some(b) => b,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "missing multipart field: target"})),
            )
                .into_response();
        }
    };

    let source_frame = match decode_to_bgr_frame(&source_bytes) {
        Ok(f)  => f,
        Err(e) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": format!("invalid source image: {e}")})),
            )
                .into_response();
        }
    };
    let mut target_frame = match decode_to_bgr_frame(&target_bytes) {
        Ok(f)  => f,
        Err(e) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": format!("invalid target image: {e}")})),
            )
                .into_response();
        }
    };

    // Lock detector, run detection, then drop the guard before acquiring swapper.
    let (source_face, target_face) = {
        let mut det_guard = server_state.models.detector.lock().unwrap();
        let detector = match det_guard.as_mut() {
            Some(d) => d,
            None => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({
                        "error": "models not loaded",
                        "detail": "FaceDetector ONNX model unavailable — check models_dir"
                    })),
                )
                    .into_response();
            }
        };

        let source_faces = match detector.detect(&source_frame, 0.3) {
            Ok(f)  => f,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("source detection failed: {e}")})),
                )
                    .into_response();
            }
        };

        if source_faces.is_empty() {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": "no face detected in source image"})),
            )
                .into_response();
        }

        let target_faces = match detector.detect(&target_frame, 0.3) {
            Ok(f)  => f,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("target detection failed: {e}")})),
                )
                    .into_response();
            }
        };

        if target_faces.is_empty() {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": "no face detected in target image"})),
            )
                .into_response();
        }

        // Pick the largest face in each image.
        let mut source_faces = source_faces;
        source_faces.sort_by(|a, b| {
            let area_a = (a.bbox[2] - a.bbox[0]) * (a.bbox[3] - a.bbox[1]);
            let area_b = (b.bbox[2] - b.bbox[0]) * (b.bbox[3] - b.bbox[1]);
            area_b.partial_cmp(&area_a).unwrap_or(std::cmp::Ordering::Equal)
        });
        let sf = source_faces.into_iter().next().unwrap();
        let mut target_faces = target_faces;
        target_faces.sort_by(|a, b| {
            let area_a = (a.bbox[2] - a.bbox[0]) * (a.bbox[3] - a.bbox[1]);
            let area_b = (b.bbox[2] - b.bbox[0]) * (b.bbox[3] - b.bbox[1]);
            area_b.partial_cmp(&area_a).unwrap_or(std::cmp::Ordering::Equal)
        });
        let tf = target_faces.into_iter().next().unwrap();
        (sf, tf)
    }; // detector guard dropped here

    let mut swap_guard = server_state.models.swapper.lock().unwrap();
    let swapper = match swap_guard.as_mut() {
        Some(s) => s,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "models not loaded",
                    "detail": "FaceSwapper ONNX models unavailable — check models_dir"
                })),
            )
                .into_response();
        }
    };

    let embedding = match swapper.get_embedding(&source_frame, &source_face) {
        Ok(e)  => e,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("embedding extraction failed: {e}")})),
            )
                .into_response();
        }
    };

    let mut source_face_with_emb = source_face;
    source_face_with_emb.embedding = Some(embedding);

    if let Err(e) = swapper.swap(&source_face_with_emb, &target_face, &mut target_frame) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("face swap failed: {e}")})),
        )
            .into_response();
    }

    let jpeg_bytes = match encode_bgr_frame_to_jpeg(&target_frame) {
        Ok(b)  => b,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("JPEG encoding failed: {e}")})),
            )
                .into_response();
        }
    };

    tracing::info!("swap_image: returning {} byte JPEG", jpeg_bytes.len());

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/jpeg")],
        Bytes::from(jpeg_bytes),
    )
        .into_response()
}

async fn list_cameras(
    _state: State<Arc<RwLock<AppState>>>,
) -> impl IntoResponse {
    // Camera probing can block for seconds per index on Windows; run off the async runtime.
    let cameras = tokio::task::spawn_blocking(|| {
        dlc_capture::list_cameras().unwrap_or_default()
    }).await.unwrap_or_default();
    Json(serde_json::json!({"cameras": cameras}))
}

async fn set_camera(
    State(state): State<Arc<RwLock<AppState>>>,
    Path(index): Path<u32>,
) -> impl IntoResponse {
    // Camera probing can block; run off the async runtime (same as list_cameras).
    let cameras = tokio::task::spawn_blocking(|| {
        dlc_capture::list_cameras().unwrap_or_default()
    }).await.unwrap_or_default();
    if !cameras.iter().any(|c| c.index == index) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Camera {} not available", index)})),
        )
            .into_response();
    }

    let mut s = state.write().unwrap();
    s.active_camera = index;
    Json(serde_json::json!({"status": "ok", "camera_index": index})).into_response()
}

async fn get_settings(
    State(state): State<Arc<RwLock<AppState>>>,
) -> impl IntoResponse {
    let s = state.read().unwrap();
    Json(serde_json::json!({
        "fp_ui": {
            "face_enhancer":         s.face_enhancer_gfpgan,
            "face_enhancer_gpen256": s.face_enhancer_gpen256,
            "face_enhancer_gpen512": s.face_enhancer_gpen512,
        },
        "frame_processors": s.frame_processors,
        "models_dir":        s.models_dir,
        "source_loaded":     s.source_image_bytes.is_some(),
        "resolution": {
            "width":  s.resolution.0,
            "height": s.resolution.1,
        },
        "swap_offset_x": s.swap_offset_x,
        "swap_offset_y": s.swap_offset_y,
        "swap_scale":    s.swap_scale,
        "detection_interval": s.detection_interval,
    }))
}

#[derive(Deserialize)]
struct SettingsUpdate {
    face_enhancer:         Option<bool>,
    face_enhancer_gpen256: Option<bool>,
    face_enhancer_gpen512: Option<bool>,
    resolution_width:      Option<u32>,
    resolution_height:     Option<u32>,
    swap_offset_x:         Option<f32>,
    swap_offset_y:         Option<f32>,
    swap_scale:            Option<f32>,
    detection_interval:    Option<u32>,
}

async fn update_settings(
    State(state): State<Arc<RwLock<AppState>>>,
    Json(body): Json<SettingsUpdate>,
) -> impl IntoResponse {
    let mut s = state.write().unwrap();
    if let Some(v) = body.face_enhancer         { s.face_enhancer_gfpgan   = v; }
    if let Some(v) = body.face_enhancer_gpen256  { s.face_enhancer_gpen256  = v; }
    if let Some(v) = body.face_enhancer_gpen512  { s.face_enhancer_gpen512  = v; }
    if let (Some(w), Some(h)) = (body.resolution_width, body.resolution_height) {
        s.resolution = (w, h);
    }
    if let Some(v) = body.swap_offset_x { s.swap_offset_x = v; }
    if let Some(v) = body.swap_offset_y { s.swap_offset_y = v; }
    if let Some(v) = body.swap_scale    { s.swap_scale = v.clamp(0.5, 2.0); }
    if let Some(v) = body.detection_interval { s.detection_interval = v.clamp(1, 30); }
    Json(serde_json::json!({"status": "ok"}))
}

async fn refresh_cameras() -> impl IntoResponse {
    let cameras = tokio::task::spawn_blocking(|| {
        dlc_capture::list_cameras().unwrap_or_default()
    }).await.unwrap_or_default();
    Json(serde_json::json!({"cameras": cameras}))
}

async fn ws_video(
    ws: WebSocketUpgrade,
    State(server_state): State<ServerState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_video_ws(socket, server_state))
}

async fn ws_metrics(
    ws: WebSocketUpgrade,
    State(server_state): State<ServerState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_metrics_ws(socket, server_state.metrics_tx.subscribe()))
}

/// WebSocket handler that forwards FrameMetrics JSON to subscribed clients.
async fn handle_metrics_ws(
    mut socket: WebSocket,
    mut rx: tokio::sync::broadcast::Receiver<String>,
) {
    use axum::extract::ws::Message;

    tracing::info!("[WS] metrics client connected");

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(json) => {
                        if let Err(e) = socket.send(Message::Text(json.into())).await {
                            tracing::info!("[WS] metrics client disconnected: {e}");
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("[WS] metrics receiver lagged by {n} messages");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    None | Some(Ok(Message::Close(_))) => {
                        tracing::info!("[WS] metrics client closed connection");
                        break;
                    }
                    Some(Err(e)) => { tracing::warn!("[WS] metrics receive error: {e}"); break; }
                    Some(Ok(_))  => {}
                }
            }
        }
    }

    tracing::info!("[WS] metrics handler exiting");
}

/// Pre-allocated placeholder frame (640x480 dark teal) shown when VideoFile mode is active
/// but live opencv decoding is not compiled in.
fn generate_video_placeholder_frame() -> &'static [u8] {
    use std::sync::OnceLock;
    static FRAME: OnceLock<Vec<u8>> = OnceLock::new();
    FRAME.get_or_init(|| {
        const W: usize = 640;
        const H: usize = 480;
        let mut pixels = vec![0u8; W * H * 3];
        for chunk in pixels.chunks_exact_mut(3) {
            chunk[0] = 0;   // R
            chunk[1] = 100; // G
            chunk[2] = 120; // B  — dark teal to distinguish from camera fallback
        }
        pixels
    })
}

/// Pre-allocated test frame (640x480 solid blue). Fallback when camera is unavailable.
fn generate_test_frame() -> &'static [u8] {
    use std::sync::OnceLock;
    static FRAME: OnceLock<Vec<u8>> = OnceLock::new();
    FRAME.get_or_init(|| {
        const W: usize = 640;
        const H: usize = 480;
        let mut pixels = vec![0u8; W * H * 3];
        for chunk in pixels.chunks_exact_mut(3) {
            chunk[2] = 200; // blue channel
        }
        pixels
    })
}

fn encode_jpeg(width: u32, height: u32, rgb_pixels: &[u8]) -> anyhow::Result<Vec<u8>> {
    use image::{ImageBuffer, Rgb, ImageEncoder};
    use image::codecs::jpeg::JpegEncoder;

    let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_raw(width, height, rgb_pixels.to_vec())
            .ok_or_else(|| anyhow::anyhow!("invalid pixel buffer dimensions"))?;

    let mut buf: Vec<u8> = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut buf, 80);
    encoder.write_image(
        img.as_raw(),
        width,
        height,
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(buf)
}

async fn handle_video_ws(mut socket: WebSocket, state: ServerState) {
    use axum::extract::ws::Message;

    state.connected_clients.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let client_count = state.connected_clients.load(std::sync::atomic::Ordering::Relaxed);
    tracing::info!("[WS] video client connected ({client_count} total)");

    // Dedicated producer thread: reads camera, runs inference, sends JPEG via channel.
    // This avoids spawn_blocking pool exhaustion and ensures sequential camera access.
    let (frame_tx, mut frame_rx) = tokio::sync::mpsc::channel::<(Vec<u8>, FrameMetrics)>(2);
    let camera = state.camera.clone();
    let st = state.clone();

    let producer = std::thread::spawn(move || {
        tracing::info!("[WS] producer thread started");
        // Face tracker: reuses cached detections to skip expensive SCRFD.
        // Interval read from AppState.detection_interval (default 10).
        let interval = st.app.read().unwrap().detection_interval;
        let mut tracker = dlc_core::tracker::FaceTracker::new(interval);
        loop {
            // Update interval from UI slider (cheap read lock).
            let new_interval = st.app.read().unwrap().detection_interval;
            tracker.set_interval(new_interval);

            match produce_frame(&camera, &st, &mut tracker) {
                Some(frame_data) => {
                    if frame_tx.blocking_send(frame_data).is_err() {
                        break; // receiver dropped — WS disconnected
                    }
                }
                None => {
                    std::thread::sleep(std::time::Duration::from_millis(33));
                }
            }
        }
        tracing::info!("[WS] producer thread exiting");
    });

    loop {
        tokio::select! {
            frame = frame_rx.recv() => {
                match frame {
                    Some((jpeg, metrics)) => {
                        if let Ok(json) = serde_json::to_string(&metrics) {
                            let _ = state.metrics_tx.send(json);
                        }
                        if let Err(e) = socket.send(Message::Binary(jpeg.into())).await {
                            tracing::info!("[WS] client disconnected: {e}");
                            break;
                        }
                    }
                    None => break, // producer exited
                }
            }

            msg = socket.recv() => {
                match msg {
                    None | Some(Ok(Message::Close(_))) => {
                        tracing::info!("[WS] client closed connection");
                        break;
                    }
                    Some(Err(e)) => { tracing::warn!("[WS] receive error: {e}"); break; }
                    Some(Ok(_))  => {}
                }
            }
        }
    }

    drop(frame_rx); // signal producer to stop
    let _ = producer.join();
    state.connected_clients.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    let remaining = state.connected_clients.load(std::sync::atomic::Ordering::Relaxed);
    tracing::info!("[WS] video handler exiting ({remaining} clients remain)");
}

/// Produce one JPEG frame with timing metrics (blocking).
/// Reads camera, optionally swaps face, encodes JPEG.
/// Returns None if encoding fails (caller skips the frame).
fn produce_frame(
    camera: &Arc<std::sync::Mutex<Option<dlc_capture::CameraCapture>>>,
    state: &ServerState,
    tracker: &mut dlc_core::tracker::FaceTracker,
) -> Option<(Vec<u8>, FrameMetrics)> {
    let total_start = std::time::Instant::now();

    // If input_mode == VideoFile, live streaming is not yet supported (requires
    // opencv).  Return a static placeholder frame so the WS connection stays
    // alive and the UI can display the "video file loaded" state.
    {
        let app = state.app.read().unwrap();
        if app.input_mode == InputMode::VideoFile {
            let rgb = generate_video_placeholder_frame();
            let jpeg = encode_jpeg(640, 480, rgb).ok()?;
            let metrics = FrameMetrics {
                total_ms: total_start.elapsed().as_secs_f64() * 1000.0,
                ..Default::default()
            };
            // Throttle to ~10 fps for the placeholder.
            std::thread::sleep(std::time::Duration::from_millis(100));
            return Some((jpeg, metrics));
        }
    }

    // Grab camera frame or fall back to test frame.
    let bgr_frame = {
        let mut cam_guard = camera.lock().unwrap();
        cam_guard.as_mut().and_then(|c| c.read_frame().ok())
    };

    let bgr_frame = match bgr_frame {
        Some(f) => f,
        None => {
            // Fallback: test frame — no swap, zero metrics.
            let rgb = generate_test_frame();
            match encode_jpeg(640, 480, rgb) {
                Ok(jpeg) => {
                    let metrics = FrameMetrics {
                        total_ms: total_start.elapsed().as_secs_f64() * 1000.0,
                        ..Default::default()
                    };
                    return Some((jpeg, metrics));
                }
                Err(e) => {
                    tracing::error!("[WS] test frame encode failed: {e}");
                    return None;
                }
            }
        }
    };

    // Read source face + cached embedding + enhancer settings in a single lock.
    let (source_bytes, cached_source, use_gfpgan, use_gpen256, use_gpen512, offset_x, offset_y, swap_scale) = {
        let app = state.app.read().unwrap();
        (
            app.source_image_bytes.clone(),
            // Use cached face+embedding if available (skips ~35ms re-detection per frame).
            app.source_face.clone(),
            app.face_enhancer_gfpgan,
            app.face_enhancer_gpen256,
            app.face_enhancer_gpen512,
            app.swap_offset_x,
            app.swap_offset_y,
            app.swap_scale,
        )
    };

    let (mut output_frame, metrics) = if let Some(src_bytes) = source_bytes {
        match try_swap_frame_sync(&bgr_frame, &src_bytes, cached_source, &state.models, offset_x, offset_y, swap_scale, tracker) {
            Some((swapped, face_rects, swap_bbox, detect_ms, swap_ms)) => {
                let face_count = face_rects.len();
                let metrics = FrameMetrics {
                    detect_ms,
                    swap_ms,
                    total_ms: 0.0,
                    face_count,
                    faces: face_rects,
                    swap_bbox: Some(swap_bbox),
                };
                (swapped, metrics)
            }
            None => {
                let metrics = FrameMetrics {
                    total_ms: total_start.elapsed().as_secs_f64() * 1000.0,
                    ..Default::default()
                };
                (bgr_frame, metrics)
            }
        }
    } else {
        let metrics = FrameMetrics {
            total_ms: total_start.elapsed().as_secs_f64() * 1000.0,
            ..Default::default()
        };
        (bgr_frame, metrics)
    };

    // Apply face enhancement if enabled and faces were detected.
    if !metrics.faces.is_empty() {
        let bbox = &[
            metrics.faces[0].x,
            metrics.faces[0].y,
            metrics.faces[0].x + metrics.faces[0].w,
            metrics.faces[0].y + metrics.faces[0].h,
        ];

        if use_gfpgan {
            if let Ok(mut guard) = state.models.enhancer_gfpgan.lock() {
                if let Some(enhancer) = guard.as_mut() {
                    let _ = enhancer.enhance(&mut output_frame, bbox);
                }
            }
        }
        if use_gpen256 {
            if let Ok(mut guard) = state.models.enhancer_gpen256.lock() {
                if let Some(enhancer) = guard.as_mut() {
                    let _ = enhancer.enhance(&mut output_frame, bbox);
                }
            }
        }
        if use_gpen512 {
            if let Ok(mut guard) = state.models.enhancer_gpen512.lock() {
                if let Some(enhancer) = guard.as_mut() {
                    let _ = enhancer.enhance(&mut output_frame, bbox);
                }
            }
        }
    }

    let mut metrics = metrics;
    metrics.total_ms = total_start.elapsed().as_secs_f64() * 1000.0;

    // Write frame to VideoWriter if recording is active (opencv feature only).
    #[cfg(feature = "opencv")]
    {
        let is_recording = state.app.read().unwrap().recording;
        if is_recording {
            if let Ok(mut writer_guard) = state.recording_writer.lock() {
                if let Some(writer) = writer_guard.as_mut() {
                    // Convert our ndarray BGR frame to an opencv Mat and write it.
                    let (h, w, _) = output_frame.dim();
                    let raw: Vec<u8> = output_frame.iter().cloned().collect();
                    if let Ok(mat) = unsafe {
                        opencv::core::Mat::new_rows_cols_with_data_unsafe(
                            h as i32, w as i32,
                            opencv::core::CV_8UC3,
                            raw.as_ptr() as *mut std::ffi::c_void,
                            opencv::core::Mat::AUTO_STEP,
                        )
                    } {
                        let _ = writer.write(&mat);
                    }
                }
            }
        }
    }

    let jpeg = encode_bgr_frame_to_jpeg(&output_frame).ok()?;
    Some((jpeg, metrics))
}

/// Synchronous face swap with timing (runs on blocking thread).
/// Returns (swapped_frame, face_rects, swap_bbox, detect_ms, swap_ms).
///
/// When `cached_source` is `Some`, source image decoding, detection, and
/// embedding extraction are all skipped — the cached face (with pre-computed
/// embedding) is used directly. This eliminates ~35 ms of redundant work per
/// frame for profile-activated sources.
fn try_swap_frame_sync(
    target_frame: &Frame,
    source_bytes: &[u8],
    cached_source: Option<dlc_core::DetectedFace>,
    models: &Arc<Models>,
    offset_x: f32,
    offset_y: f32,
    scale: f32,
    tracker: &mut dlc_core::tracker::FaceTracker,
) -> Option<(Frame, Vec<FaceRect>, FaceRect, f64, f64)> {
    let detect_start = std::time::Instant::now();

    // --- Target face: use tracker to skip detection on most frames ---
    let target_faces = if tracker.should_detect() {
        // Full detection (runs every Nth frame).
        let mut det_guard = models.detector.lock().ok()?;
        let detector = det_guard.as_mut()?;
        let mut tfs = detector.detect(target_frame, 0.3).ok()?;
        tfs.sort_by(|a, b| {
            let area_a = (a.bbox[2] - a.bbox[0]) * (a.bbox[3] - a.bbox[1]);
            let area_b = (b.bbox[2] - b.bbox[0]) * (b.bbox[3] - b.bbox[1]);
            area_b.partial_cmp(&area_a).unwrap_or(std::cmp::Ordering::Equal)
        });
        tracker.update_detected(tfs.clone());
        tfs
    } else {
        // Skip detection — reuse cached target faces from last detection.
        tracker.get_cached().to_vec()
    };

    if target_faces.is_empty() {
        tracker.invalidate(); // lost faces → force re-detect next frame
        return None;
    }
    let target_face = target_faces[0].clone();

    // --- Source face: use cached embedding or decode+detect ---
    let source_face = if let Some(cached) = cached_source {
        cached
    } else {
        let source_frame = decode_to_bgr_frame(source_bytes).ok()?;
        let mut det_guard = models.detector.lock().ok()?;
        let detector = det_guard.as_mut()?;
        let mut sfs = detector.detect(&source_frame, 0.3).ok()?;
        sfs.sort_by(|a, b| {
            let area_a = (a.bbox[2] - a.bbox[0]) * (a.bbox[3] - a.bbox[1]);
            let area_b = (b.bbox[2] - b.bbox[0]) * (b.bbox[3] - b.bbox[1]);
            area_b.partial_cmp(&area_a).unwrap_or(std::cmp::Ordering::Equal)
        });
        sfs.into_iter().next()?
    };

    let detect_ms = detect_start.elapsed().as_secs_f64() * 1000.0;

    // Build FaceRect list from detected target faces.
    let face_rects: Vec<FaceRect> = target_faces
        .iter()
        .map(|f| FaceRect {
            x: f.bbox[0],
            y: f.bbox[1],
            w: f.bbox[2] - f.bbox[0],
            h: f.bbox[3] - f.bbox[1],
            score: f.score,
        })
        .collect();

    // Apply calibration: offset + scale the target face landmarks.
    let mut calibrated_target = target_face.clone();
    if offset_x != 0.0 || offset_y != 0.0 || scale != 1.0 {
        // Compute face center from landmarks
        let cx: f32 = calibrated_target.landmarks.iter().map(|l| l[0]).sum::<f32>() / 5.0;
        let cy: f32 = calibrated_target.landmarks.iter().map(|l| l[1]).sum::<f32>() / 5.0;
        for lm in calibrated_target.landmarks.iter_mut() {
            // Scale around center, then offset
            lm[0] = (lm[0] - cx) * scale + cx + offset_x;
            lm[1] = (lm[1] - cy) * scale + cy + offset_y;
        }
        // Also adjust bbox
        let bw = calibrated_target.bbox[2] - calibrated_target.bbox[0];
        let bh = calibrated_target.bbox[3] - calibrated_target.bbox[1];
        let bcx = (calibrated_target.bbox[0] + calibrated_target.bbox[2]) / 2.0;
        let bcy = (calibrated_target.bbox[1] + calibrated_target.bbox[3]) / 2.0;
        calibrated_target.bbox[0] = bcx - bw * scale / 2.0 + offset_x;
        calibrated_target.bbox[1] = bcy - bh * scale / 2.0 + offset_y;
        calibrated_target.bbox[2] = bcx + bw * scale / 2.0 + offset_x;
        calibrated_target.bbox[3] = bcy + bh * scale / 2.0 + offset_y;
    }

    // Swap face, timed.
    // If the cached source already carries an embedding, use it directly;
    // otherwise compute it now (requires decoding source bytes again).
    let swap_start = std::time::Instant::now();
    let mut swap_guard = models.swapper.lock().ok()?;
    let swapper = swap_guard.as_mut()?;
    let sf = if source_face.embedding.is_some() {
        source_face
    } else {
        let source_frame = decode_to_bgr_frame(source_bytes).ok()?;
        let embedding = swapper.get_embedding(&source_frame, &source_face).ok()?;
        let mut face = source_face;
        face.embedding = Some(embedding);
        face
    };
    // Ensure embedding field is set (satisfies swap contract).
    let _ = sf.embedding.as_ref()?;
    let mut output = target_frame.clone();
    swapper.swap(&sf, &calibrated_target, &mut output).ok()?;
    let swap_ms = swap_start.elapsed().as_secs_f64() * 1000.0;

    // Swap bbox for the red overlay
    let swap_bbox = FaceRect {
        x: calibrated_target.bbox[0],
        y: calibrated_target.bbox[1],
        w: calibrated_target.bbox[2] - calibrated_target.bbox[0],
        h: calibrated_target.bbox[3] - calibrated_target.bbox[1],
        score: calibrated_target.score,
    };

    Some((output, face_rects, swap_bbox, detect_ms, swap_ms))
}

// ---------------------------------------------------------------------------
// Provider detection helpers
// ---------------------------------------------------------------------------

/// Map a `selected_provider` string back to a `GpuProvider`.
fn provider_from_str(s: &str) -> GpuProvider {
    match s {
        "DirectML" => GpuProvider::DirectML { device_id: 0 },
        "NPU" => GpuProvider::Npu {
            config_file: std::env::var("DEEP_FORGE_NPU_CONFIG")
                .unwrap_or_else(|_| "vaip_config.json".into()),
            cache_dir: std::env::var("DEEP_FORGE_NPU_CACHE")
                .unwrap_or_else(|_| "./npu_cache".into()),
        },
        "CPU" => GpuProvider::Cpu,
        _ => GpuProvider::detect(), // "Auto" or unknown
    }
}

/// Detect whether the VitisAI NPU EP is likely available.
/// Checks for `vaip_config.json` next to the server executable.
fn npu_available() -> bool {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let config_path = exe_dir.join("vaip_config.json");
    config_path.exists()
}

// ---------------------------------------------------------------------------
// GET /providers
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct ProviderInfo {
    name: String,
    available: bool,
    description: String,
}

#[derive(serde::Serialize)]
struct ProvidersResponse {
    active: String,
    available: Vec<ProviderInfo>,
}

async fn list_providers(State(state): State<ServerState>) -> impl IntoResponse {
    // Determine what is actually running (gpu_provider set at startup).
    let active = state.gpu_provider.clone();

    let providers = vec![
        ProviderInfo {
            name: "DirectML".to_string(),
            available: true, // always available on Windows via ort
            description: "AMD/Intel/NVIDIA GPU via DirectML".to_string(),
        },
        ProviderInfo {
            name: "VitisAI NPU".to_string(),
            available: npu_available(),
            description: "AMD XDNA2 NPU (Ryzen AI)".to_string(),
        },
        ProviderInfo {
            name: "CPU".to_string(),
            available: true,
            description: "CPU fallback (always available)".to_string(),
        },
    ];

    Json(ProvidersResponse {
        active,
        available: providers,
    })
}

// ---------------------------------------------------------------------------
// POST /providers/switch
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SwitchProviderRequest {
    provider: String,
}

async fn switch_provider(
    State(state): State<ServerState>,
    Json(body): Json<SwitchProviderRequest>,
) -> impl IntoResponse {
    let valid = ["Auto", "DirectML", "NPU", "CPU"];
    if !valid.contains(&body.provider.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!(
                    "unknown provider '{}'; valid values: Auto, DirectML, NPU, CPU",
                    body.provider
                )
            })),
        )
            .into_response();
    }

    state.app.write().unwrap().selected_provider = body.provider.clone();
    tracing::info!("[PROVIDERS] selected_provider updated to '{}'", body.provider);

    Json(serde_json::json!({
        "status": "ok",
        "selected": body.provider,
        "message": "Provider stored. Call POST /models/reload to apply.",
    }))
    .into_response()
}

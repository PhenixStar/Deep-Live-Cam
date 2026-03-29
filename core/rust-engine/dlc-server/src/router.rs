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
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use tower_http::cors::{CorsLayer, Any};

use crate::state::AppState;
use dlc_core::{detect::FaceDetector, swap::FaceSwapper, Frame};

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
}

impl Default for FrameMetrics {
    fn default() -> Self {
        Self {
            detect_ms: 0.0,
            swap_ms: 0.0,
            total_ms: 0.0,
            face_count: 0,
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
}

// ---------------------------------------------------------------------------
// Combined server state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ServerState {
    pub app:            Arc<RwLock<AppState>>,
    pub models:         Arc<Models>,
    pub metrics_tx:     tokio::sync::broadcast::Sender<String>,
    pub gpu_provider:   String,
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
pub fn build_router(server_state: ServerState) -> Router {
    // Tauri v2 uses http://tauri.localhost on Windows, tauri://localhost on macOS/Linux
    let cors = CorsLayer::new()
        .allow_origin([
            "tauri://localhost".parse().unwrap(),
            "http://tauri.localhost".parse().unwrap(),
            "https://tauri.localhost".parse().unwrap(),
            "http://localhost:1420".parse().unwrap(),
            "http://localhost:8008".parse().unwrap(),
        ])
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/health",         get(health))
        .route("/source",         post(upload_source))
        .route("/swap/image",     post(swap_image))
        .route("/cameras",        get(list_cameras))
        .route("/camera/{index}", post(set_camera))
        .route("/settings",       get(get_settings).post(update_settings))
        .route("/ws/video",       get(ws_video))
        .route("/ws/metrics",     get(ws_metrics))
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(cors)
        .with_state(server_state)
}

/// Build a `ServerState` with no models loaded (safe for unit/integration tests).
pub fn test_state() -> ServerState {
    let (metrics_tx, _) = tokio::sync::broadcast::channel(64);
    ServerState {
        app:          Arc::new(RwLock::new(AppState::default())),
        models:       Arc::new(Models {
            detector: Mutex::new(None),
            swapper:  Mutex::new(None),
        }),
        metrics_tx,
        gpu_provider: "Auto".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn health(State(server_state): State<ServerState>) -> impl IntoResponse {
    let models = &server_state.models;
    let detector_ok = models.detector.lock().map(|g| g.is_some()).unwrap_or(false);
    let swapper_ok  = models.swapper.lock().map(|g| g.is_some()).unwrap_or(false);

    Json(serde_json::json!({
        "status": "ok",
        "backend": "rust",
        "gpu_provider": server_state.gpu_provider,
        "models": {
            "detector": detector_ok,
            "swapper":  swapper_ok,
        }
    }))
}

async fn upload_source(
    State(state): State<Arc<RwLock<AppState>>>,
    mut multipart: axum::extract::Multipart,
) -> impl IntoResponse {
    let field = loop {
        match multipart.next_field().await {
            Ok(Some(f)) => break f,
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

    let mut s = state.write().await;
    s.source_image_bytes = Some(bytes.to_vec());
    s.source_face = None;

    Json(serde_json::json!({"status": "ok", "bytes": bytes.len()})).into_response()
}

// ---------------------------------------------------------------------------
// Image helpers used by swap_image
// ---------------------------------------------------------------------------

fn decode_to_bgr_frame(bytes: &[u8]) -> anyhow::Result<Frame> {
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

        let sf = source_faces.into_iter().next().unwrap();
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
    let cameras = dlc_capture::list_cameras().unwrap_or_default();
    if !cameras.iter().any(|c| c.index == index) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Camera {} not available", index)})),
        )
            .into_response();
    }

    let mut s = state.write().await;
    s.active_camera = index;
    Json(serde_json::json!({"status": "ok", "camera_index": index})).into_response()
}

async fn get_settings(
    State(state): State<Arc<RwLock<AppState>>>,
) -> impl IntoResponse {
    let s = state.read().await;
    Json(serde_json::json!({
        "fp_ui": {
            "face_enhancer":       s.face_enhancer_gfpgan,
            "face_enhancer_gpen256": s.face_enhancer_gpen256,
            "face_enhancer_gpen512": s.face_enhancer_gpen512,
        },
        "frame_processors": s.frame_processors,
        "models_dir":        s.models_dir,
        "source_loaded":     s.source_image_bytes.is_some(),
    }))
}

#[derive(Deserialize)]
struct SettingsUpdate {
    face_enhancer:        Option<bool>,
    face_enhancer_gpen256: Option<bool>,
    face_enhancer_gpen512: Option<bool>,
}

async fn update_settings(
    State(state): State<Arc<RwLock<AppState>>>,
    Json(body): Json<SettingsUpdate>,
) -> impl IntoResponse {
    let mut s = state.write().await;
    if let Some(v) = body.face_enhancer         { s.face_enhancer_gfpgan   = v; }
    if let Some(v) = body.face_enhancer_gpen256  { s.face_enhancer_gpen256  = v; }
    if let Some(v) = body.face_enhancer_gpen512  { s.face_enhancer_gpen512  = v; }
    Json(serde_json::json!({"status": "ok"}))
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

    tracing::info!("[WS] video client connected");

    // Open camera on a blocking thread (OpenCV calls block).
    let camera_index = { state.app.read().await.active_camera };
    let camera = tokio::task::spawn_blocking(move || {
        dlc_capture::CameraCapture::open(camera_index).ok()
    }).await.unwrap_or(None);

    let camera = Arc::new(std::sync::Mutex::new(camera));

    if camera.lock().unwrap().is_none() {
        tracing::warn!("[WS] Camera {camera_index} unavailable — sending test frames");
    }

    let mut ticker = tokio::time::interval(tokio::time::Duration::from_millis(33));

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let cam = camera.clone();
                let st = state.clone();

                // All blocking work (camera read + inference) runs off the async runtime.
                let frame_result = tokio::task::spawn_blocking(move || {
                    produce_frame(&cam, &st)
                }).await;

                let (jpeg, metrics) = match frame_result {
                    Ok(Some(r)) => r,
                    Ok(None) => continue, // encode error, skip frame
                    Err(e) => { tracing::error!("[WS] task panic: {e}"); continue; }
                };

                if let Err(e) = socket.send(Message::Binary(jpeg.into())).await {
                    tracing::info!("[WS] client disconnected: {e}");
                    break;
                }

                // Broadcast metrics JSON (best-effort; ignore send errors when no subscribers).
                if let Ok(json) = serde_json::to_string(&metrics) {
                    let _ = state.metrics_tx.send(json);
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

    tracing::info!("[WS] video handler exiting");
}

/// Produce one JPEG frame with timing metrics (blocking).
/// Reads camera, optionally swaps face, encodes JPEG.
/// Returns None if encoding fails (caller skips the frame).
fn produce_frame(
    camera: &Arc<std::sync::Mutex<Option<dlc_capture::CameraCapture>>>,
    state: &ServerState,
) -> Option<(Vec<u8>, FrameMetrics)> {
    let total_start = std::time::Instant::now();

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
            let jpeg = encode_jpeg(640, 480, rgb).ok()?;
            let metrics = FrameMetrics {
                total_ms: total_start.elapsed().as_secs_f64() * 1000.0,
                ..Default::default()
            };
            return Some((jpeg, metrics));
        }
    };

    // Check if source face is uploaded — if so, try swap.
    let source_bytes = {
        let app = state.app.blocking_read();
        app.source_image_bytes.clone()
    };

    let (output_frame, metrics) = if let Some(src_bytes) = source_bytes {
        match try_swap_frame_sync(&bgr_frame, &src_bytes, &state.models) {
            Some((swapped, face_rects, detect_ms, swap_ms)) => {
                let face_count = face_rects.len();
                let metrics = FrameMetrics {
                    detect_ms,
                    swap_ms,
                    total_ms: total_start.elapsed().as_secs_f64() * 1000.0,
                    face_count,
                    faces: face_rects,
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

    let jpeg = encode_bgr_frame_to_jpeg(&output_frame).ok()?;
    Some((jpeg, metrics))
}

/// Synchronous face swap with timing (runs on blocking thread).
/// Returns (swapped_frame, face_rects, detect_ms, swap_ms) on success.
fn try_swap_frame_sync(
    target_frame: &Frame,
    source_bytes: &[u8],
    models: &Arc<Models>,
) -> Option<(Frame, Vec<FaceRect>, f64, f64)> {
    let source_frame = decode_to_bgr_frame(source_bytes).ok()?;

    // Detect faces in source and target, timed.
    let detect_start = std::time::Instant::now();
    let (source_face, target_face, target_faces) = {
        let mut det_guard = models.detector.lock().ok()?;
        let detector = det_guard.as_mut()?;
        let sf = detector.detect(&source_frame, 0.3).ok()?.into_iter().next()?;
        let tfs = detector.detect(target_frame, 0.3).ok()?;
        let tf = tfs.iter().next()?.clone();
        (sf, tf, tfs)
    }; // detector guard dropped
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

    // Swap face, timed.
    let swap_start = std::time::Instant::now();
    let mut swap_guard = models.swapper.lock().ok()?;
    let swapper = swap_guard.as_mut()?;
    let embedding = swapper.get_embedding(&source_frame, &source_face).ok()?;
    let mut sf = source_face;
    sf.embedding = Some(embedding);
    let mut output = target_frame.clone();
    swapper.swap(&sf, &target_face, &mut output).ok()?;
    let swap_ms = swap_start.elapsed().as_secs_f64() * 1000.0;

    Some((output, face_rects, detect_ms, swap_ms))
}

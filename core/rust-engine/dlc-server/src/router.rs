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
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tower_http::cors::{CorsLayer, Any};

use crate::state::AppState;
use dlc_core::{detect::FaceDetector, swap::FaceSwapper, Frame};

// ---------------------------------------------------------------------------
// Model container
// ---------------------------------------------------------------------------

pub struct Models {
    pub detector: Arc<Mutex<Option<FaceDetector>>>,
    pub swapper:  Arc<Mutex<Option<FaceSwapper>>>,
}

// ---------------------------------------------------------------------------
// Combined server state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ServerState {
    pub app:    Arc<RwLock<AppState>>,
    pub models: Arc<Models>,
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
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(cors)
        .with_state(server_state)
}

/// Build a `ServerState` with no models loaded (safe for unit/integration tests).
pub fn test_state() -> ServerState {
    ServerState {
        app:    Arc::new(RwLock::new(AppState::default())),
        models: Arc::new(Models {
            detector: Arc::new(Mutex::new(None)),
            swapper:  Arc::new(Mutex::new(None)),
        }),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok", "backend": "rust"}))
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
        let mut det_guard = server_state.models.detector.lock().await;
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

        let source_faces = match detector.detect(&source_frame, 0.5) {
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

        let target_faces = match detector.detect(&target_frame, 0.5) {
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

    let mut swap_guard = server_state.models.swapper.lock().await;
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
    use tokio::time::{interval, Duration};

    tracing::info!("[WS] video client connected");
    let mut ticker = interval(Duration::from_millis(33));

    // Open camera; fall back to test frames if unavailable.
    let camera_index = { state.app.read().await.active_camera };
    let mut camera = dlc_capture::CameraCapture::open(camera_index).ok();
    if camera.is_none() {
        tracing::warn!("[WS] Camera {camera_index} unavailable — sending test frames");
    }
    let mut last_camera_index = camera_index;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                // Re-open camera if user switched it.
                let current_idx = { state.app.read().await.active_camera };
                if current_idx != last_camera_index {
                    camera = dlc_capture::CameraCapture::open(current_idx).ok();
                    last_camera_index = current_idx;
                    if camera.is_none() {
                        tracing::warn!("[WS] Camera {current_idx} unavailable after switch");
                    }
                }

                // Grab a frame from camera or generate a test frame.
                let bgr_frame = match camera.as_mut().and_then(|c| c.read_frame().ok()) {
                    Some(f) => f,
                    None => {
                        // Fallback: send test frame as JPEG.
                        let rgb = generate_test_frame();
                        let jpeg = match encode_jpeg(640, 480, rgb) {
                            Ok(j)  => j,
                            Err(e) => { tracing::error!("[WS] JPEG encode error: {e}"); continue; }
                        };
                        if let Err(e) = socket.send(Message::Binary(jpeg.into())).await {
                            tracing::info!("[WS] client disconnected: {e}");
                            break;
                        }
                        continue;
                    }
                };

                // If a source face is uploaded, attempt face swap on the camera frame.
                let has_source = { state.app.read().await.source_image_bytes.is_some() };
                let output_frame = if has_source {
                    match try_swap_frame(&bgr_frame, &state).await {
                        Some(swapped) => swapped,
                        None => bgr_frame, // swap failed — send raw camera frame
                    }
                } else {
                    bgr_frame
                };

                let jpeg = match encode_bgr_frame_to_jpeg(&output_frame) {
                    Ok(j)  => j,
                    Err(e) => { tracing::error!("[WS] JPEG encode error: {e}"); continue; }
                };

                if let Err(e) = socket.send(Message::Binary(jpeg.into())).await {
                    tracing::info!("[WS] client disconnected: {e}");
                    break;
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

/// Attempt face detection + swap on a camera frame using the uploaded source face.
/// Returns `None` if any step fails (missing models, no face detected, etc.).
async fn try_swap_frame(target_frame: &Frame, state: &ServerState) -> Option<Frame> {
    let source_bytes = { state.app.read().await.source_image_bytes.clone()? };
    let source_frame = decode_to_bgr_frame(&source_bytes).ok()?;

    // Detect faces in source and target.
    let (source_face, target_face) = {
        let mut det_guard = state.models.detector.lock().await;
        let detector = det_guard.as_mut()?;
        let sf = detector.detect(&source_frame, 0.5).ok()?.into_iter().next()?;
        let tf = detector.detect(target_frame, 0.5).ok()?.into_iter().next()?;
        (sf, tf)
    }; // detector guard dropped

    // Swap face.
    let mut swap_guard = state.models.swapper.lock().await;
    let swapper = swap_guard.as_mut()?;
    let embedding = swapper.get_embedding(&source_frame, &source_face).ok()?;
    let mut sf = source_face;
    sf.embedding = Some(embedding);
    let mut output = target_frame.clone();
    swapper.swap(&sf, &target_face, &mut output).ok()?;
    Some(output)
}

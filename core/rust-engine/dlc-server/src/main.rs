//! Deep-Live-Cam Rust server — drop-in replacement for Python FastAPI sidecar.
//!
//! Implements the same HTTP API contract so the Tauri frontend works unchanged.

use axum::{
    Router,
    extract::{Path, Json, State, ws::{WebSocket, WebSocketUpgrade}},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{CorsLayer, Any};

mod state;

use state::AppState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Parse --models-dir CLI arg, fall back to env var / default inside AppState::default().
    let models_dir = parse_models_dir_arg();

    let mut app_state = AppState::default();
    if let Some(dir) = models_dir {
        app_state.models_dir = dir;
    }

    tracing::info!("[SERVER] models_dir = {}", app_state.models_dir.display());

    let state = Arc::new(RwLock::new(app_state));

    let cors = CorsLayer::new()
        .allow_origin([
            "tauri://localhost".parse().unwrap(),
            "http://localhost:1420".parse().unwrap(),
            "http://localhost:8008".parse().unwrap(),
        ])
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(health))
        .route("/source", post(upload_source))
        .route("/swap/image", post(swap_image))
        .route("/cameras", get(list_cameras))
        .route("/camera/{index}", post(set_camera))
        .route("/settings", get(get_settings).post(update_settings))
        .route("/ws/video", get(ws_video))
        .layer(cors)
        .with_state(state);

    let addr = "127.0.0.1:8008";
    tracing::info!("[SERVER] Rust backend starting on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// Parse `--models-dir <path>` from process arguments.
fn parse_models_dir_arg() -> Option<std::path::PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let pos = args.iter().position(|a| a == "--models-dir")?;
    args.get(pos + 1).map(std::path::PathBuf::from)
}

// --- Handlers ---

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok", "backend": "rust"}))
}

/// POST /source — multipart upload of a source face image.
/// Decodes the image to validate it, then stores raw bytes in state.
/// Face detection will be wired in Week 6 once dlc-core detection is ready.
async fn upload_source(
    State(state): State<Arc<RwLock<AppState>>>,
    mut multipart: axum::extract::Multipart,
) -> impl IntoResponse {
    // Extract the first file field from the multipart body.
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

    // Validate that the upload is a readable image.
    if let Err(e) = image::load_from_memory(&bytes) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": format!("invalid image: {e}")})),
        )
            .into_response();
    }

    tracing::info!("source image received: {} bytes", bytes.len());

    // Store raw bytes; face detection wired in Week 6.
    let mut s = state.write().await;
    s.source_image_bytes = Some(bytes.to_vec());
    s.source_face = None; // reset any previous detection result

    Json(serde_json::json!({"status": "ok", "bytes": bytes.len()})).into_response()
}

/// POST /swap/image — multipart with source + target images, returns swapped JPEG.
/// Full implementation deferred to Week 6 (ONNX models not yet wired).
async fn swap_image(
    _state: State<Arc<RwLock<AppState>>>,
    _multipart: axum::extract::Multipart,
) -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "not implemented",
            "detail": "/swap/image will be fully wired in Week 6 once ONNX face-swap models are integrated"
        })),
    )
}

async fn list_cameras(
    _state: State<Arc<RwLock<AppState>>>,
) -> impl IntoResponse {
    let cameras = dlc_capture::list_cameras().unwrap_or_default();
    Json(serde_json::json!({"cameras": cameras}))
}

async fn set_camera(
    State(state): State<Arc<RwLock<AppState>>>,
    Path(index): Path<u32>,
) -> impl IntoResponse {
    // Validate against known cameras (mirrors Python behaviour).
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
            "face_enhancer": s.face_enhancer_gfpgan,
            "face_enhancer_gpen256": s.face_enhancer_gpen256,
            "face_enhancer_gpen512": s.face_enhancer_gpen512,
        },
        "frame_processors": s.frame_processors,
        "models_dir": s.models_dir,
        "source_loaded": s.source_image_bytes.is_some(),
    }))
}

#[derive(Deserialize)]
struct SettingsUpdate {
    face_enhancer: Option<bool>,
    face_enhancer_gpen256: Option<bool>,
    face_enhancer_gpen512: Option<bool>,
}

async fn update_settings(
    State(state): State<Arc<RwLock<AppState>>>,
    Json(body): Json<SettingsUpdate>,
) -> impl IntoResponse {
    let mut s = state.write().await;
    if let Some(v) = body.face_enhancer { s.face_enhancer_gfpgan = v; }
    if let Some(v) = body.face_enhancer_gpen256 { s.face_enhancer_gpen256 = v; }
    if let Some(v) = body.face_enhancer_gpen512 { s.face_enhancer_gpen512 = v; }
    Json(serde_json::json!({"status": "ok"}))
}

async fn ws_video(
    ws: WebSocketUpgrade,
    State(state): State<Arc<RwLock<AppState>>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_video_ws(socket, state))
}

/// Generate a solid-blue (R=0, G=0, B=200) RGB test frame, 640x480.
/// Replaces real camera capture until nokhwa is wired in Week 7.
fn generate_test_frame() -> Vec<u8> {
    const W: usize = 640;
    const H: usize = 480;
    let mut pixels = vec![0u8; W * H * 3];
    for chunk in pixels.chunks_exact_mut(3) {
        chunk[0] = 0;   // R
        chunk[1] = 0;   // G
        chunk[2] = 200; // B
    }
    pixels
}

/// Encode raw RGB pixels (width x height x 3) as JPEG bytes at quality 80.
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

/// WebSocket handler: streams JPEG frames at ~30 fps.
///
/// Frame source is a stub (solid-blue 640x480) until nokhwa is integrated in Week 7.
/// Face processing hook is present but no-op until dlc-core pipeline lands in Week 6.
/// Uses `tokio::select!` so incoming client messages (close frames, etc.) are handled
/// concurrently with the send path — no separate task needed.
async fn handle_video_ws(mut socket: WebSocket, state: Arc<RwLock<AppState>>) {
    use axum::extract::ws::Message;
    use tokio::time::{interval, Duration};

    tracing::info!("[WS] video client connected");

    // ~30 fps ticker (33 ms per frame).
    let mut ticker = interval(Duration::from_millis(33));

    loop {
        tokio::select! {
            // Frame tick: generate, (stub-)process, encode, send.
            _ = ticker.tick() => {
                // Stub capture: solid-blue test frame until Week 7 (nokhwa).
                let rgb = generate_test_frame();

                // Stub processing: check if a source face is set, pass frame through.
                // Real face swap / enhancement wired in Week 6-7.
                {
                    let _s = state.read().await;
                    // When _s.source_face is Some, invoke processors here.
                    // No-op until dlc-core pipeline is wired.
                }

                let jpeg = match encode_jpeg(640, 480, &rgb) {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::error!("[WS] JPEG encode error: {e}");
                        continue;
                    }
                };

                if let Err(e) = socket.send(Message::Binary(jpeg.into())).await {
                    tracing::info!("[WS] send failed (client disconnected): {e}");
                    break;
                }
            }

            // Incoming messages: axum handles ping/pong automatically;
            // a Close frame or recv error signals client disconnect.
            msg = socket.recv() => {
                match msg {
                    None | Some(Ok(Message::Close(_))) => {
                        tracing::info!("[WS] client closed connection");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::warn!("[WS] receive error: {e}");
                        break;
                    }
                    Some(Ok(_)) => {
                        // Ignore ping/pong/text/binary from client — this is a read-only stream.
                    }
                }
            }
        }
    }

    tracing::info!("[WS] video handler exiting");
}

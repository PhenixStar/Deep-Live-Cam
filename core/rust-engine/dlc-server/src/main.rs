//! Deep-Live-Cam Rust server — entry point.
//!
//! Binds the TCP socket and delegates everything else to `dlc_server::router`.

use dlc_server::router::{build_router, Models, ServerState};
use dlc_server::state::AppState;
use dlc_core::{detect::FaceDetector, swap::FaceSwapper, GpuProvider};
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let models_dir = parse_models_dir_arg();

    let mut app_state = AppState::default();
    if let Some(dir) = models_dir {
        app_state.models_dir = dir;
    }

    tracing::info!("[SERVER] models_dir = {}", app_state.models_dir.display());

    // GPU provider: try DirectML first, fall back to CPU.
    let provider = GpuProvider::Auto;
    tracing::info!("[SERVER] GPU provider: {:?}", provider);

    // Load ONNX models — optional; server starts without them and returns 503
    // on swap requests if missing.
    let det_path = app_state.models_dir.join("buffalo_l/buffalo_l/det_10g.onnx");
    let detector = match FaceDetector::new(&det_path, &provider) {
        Ok(d)  => { tracing::info!("FaceDetector loaded");             Some(d) }
        Err(e) => { tracing::warn!("FaceDetector unavailable: {e:#}"); None   }
    };
    let swapper = match FaceSwapper::new(&app_state.models_dir, &provider) {
        Ok(s)  => { tracing::info!("FaceSwapper loaded");              Some(s) }
        Err(e) => { tracing::warn!("FaceSwapper unavailable: {e:#}");  None   }
    };

    let server_state = ServerState {
        app:    Arc::new(RwLock::new(app_state)),
        models: Arc::new(Models {
            detector: std::sync::Mutex::new(detector),
            swapper:  std::sync::Mutex::new(swapper),
        }),
    };

    let app = build_router(server_state);

    let addr = "127.0.0.1:8008";
    tracing::info!("[SERVER] Rust backend starting on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn parse_models_dir_arg() -> Option<std::path::PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let pos = args.iter().position(|a| a == "--models-dir")?;
    args.get(pos + 1).map(std::path::PathBuf::from)
}

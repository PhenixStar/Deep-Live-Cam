//! Deep-Live-Cam Rust server — entry point.
//!
//! Binds the TCP socket and delegates everything else to `dlc_server::router`.

use dlc_server::router::{build_router, Models, ServerState};
use dlc_server::state::AppState;
use dlc_core::{detect::FaceDetector, swap::FaceSwapper, enhance::FaceEnhancer, GpuProvider};
use std::sync::{Arc, RwLock};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let models_dir = parse_models_dir_arg();
    let remote = parse_remote_flag();

    let mut app_state = AppState::default();
    if let Some(dir) = models_dir {
        app_state.models_dir = dir;
    }

    tracing::info!("[SERVER] models_dir = {}", app_state.models_dir.display());

    // GPU provider: try DirectML first, fall back to CPU.
    let provider = GpuProvider::Auto;
    let gpu_provider_name = format!("{:?}", provider);
    tracing::info!("[SERVER] GPU provider: {}", gpu_provider_name);

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

    // Load enhancer models (optional — face swap works without them).
    let gfpgan_path = app_state.models_dir.join("gfpgan-1024.onnx");
    let enhancer_gfpgan = match FaceEnhancer::new(&gfpgan_path, 1024, &provider) {
        Ok(e)  => { tracing::info!("GFPGAN enhancer loaded");              Some(e) }
        Err(e) => { tracing::warn!("GFPGAN enhancer unavailable: {e:#}");  None   }
    };
    let gpen256_path = app_state.models_dir.join("GPEN-BFR-256.onnx");
    let enhancer_gpen256 = match FaceEnhancer::new(&gpen256_path, 256, &provider) {
        Ok(e)  => { tracing::info!("GPEN-256 enhancer loaded");              Some(e) }
        Err(e) => { tracing::warn!("GPEN-256 enhancer unavailable: {e:#}");  None   }
    };
    let gpen512_path = app_state.models_dir.join("GPEN-BFR-512.onnx");
    let enhancer_gpen512 = match FaceEnhancer::new(&gpen512_path, 512, &provider) {
        Ok(e)  => { tracing::info!("GPEN-512 enhancer loaded");              Some(e) }
        Err(e) => { tracing::warn!("GPEN-512 enhancer unavailable: {e:#}");  None   }
    };

    // Open camera at startup on a blocking thread (can take 10-30s on Windows).
    tracing::info!("[SERVER] Opening camera 0 (may take up to 30s on Windows)...");
    let camera = {
        let cam = std::thread::spawn(|| {
            dlc_capture::CameraCapture::open(0).ok()
        }).join().unwrap_or(None);
        if cam.is_some() {
            tracing::info!("[SERVER] Camera 0 opened successfully");
        } else {
            tracing::warn!("[SERVER] Camera 0 unavailable — test frames will be used");
        }
        cam
    };

    // Broadcast channel for per-frame metrics (capacity: 64 frames).
    let (metrics_tx, _) = tokio::sync::broadcast::channel(64);

    // Generate API token when running in remote mode.
    let api_token = if remote {
        let token = generate_token();
        tracing::info!("[SERVER] Remote mode enabled. API token: {}", token);
        Some(token)
    } else {
        None
    };

    let addr = if remote { "0.0.0.0:8008" } else { "127.0.0.1:8008" };

    let server_state = ServerState {
        app:    Arc::new(RwLock::new(app_state)),
        camera: Arc::new(std::sync::Mutex::new(camera)),
        models: Arc::new(Models {
            detector: std::sync::Mutex::new(detector),
            swapper:  std::sync::Mutex::new(swapper),
            enhancer_gfpgan:  std::sync::Mutex::new(enhancer_gfpgan),
            enhancer_gpen256: std::sync::Mutex::new(enhancer_gpen256),
            enhancer_gpen512: std::sync::Mutex::new(enhancer_gpen512),
        }),
        metrics_tx,
        gpu_provider: gpu_provider_name,
        remote_mode: remote,
        bind_address: addr.to_string(),
        api_token,
    };

    let app = build_router(server_state, remote);

    tracing::info!("[SERVER] Rust backend starting on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn parse_models_dir_arg() -> Option<std::path::PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let pos = args.iter().position(|a| a == "--models-dir")?;
    args.get(pos + 1).map(std::path::PathBuf::from)
}

fn parse_remote_flag() -> bool {
    std::env::args().any(|a| a == "--remote")
}

fn generate_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let s = RandomState::new();
    let mut h = s.build_hasher();
    h.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64,
    );
    format!("{:016x}", h.finish())
}

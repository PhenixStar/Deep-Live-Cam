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

    // Limit OpenMP threads for DirectML stability on AMD GPUs (upstream PR #1710).
    if std::env::var("OMP_NUM_THREADS").is_err() {
        std::env::set_var("OMP_NUM_THREADS", "6");
    }

    let models_dir = parse_models_dir_arg();
    let remote = parse_remote_flag();

    let mut app_state = AppState::default();
    if let Some(dir) = models_dir {
        app_state.models_dir = dir;
    }

    tracing::info!("[SERVER] models_dir = {}", app_state.models_dir.display());

    // Auto-detect best GPU/accelerator. Override with DEEP_FORGE_EP env var
    // or --npu / --cuda / --cpu CLI flags.
    let provider = if parse_npu_flag() {
        GpuProvider::Npu {
            config_file: std::env::var("DEEP_FORGE_NPU_CONFIG")
                .unwrap_or_else(|_| "vaip_config.json".into()),
            cache_dir: std::env::var("DEEP_FORGE_NPU_CACHE")
                .unwrap_or_else(|_| "./npu_cache".into()),
        }
    } else {
        GpuProvider::detect()
    };
    let gpu_provider_name = provider.display_name().to_string();
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

    // Broadcast channel for per-frame metrics (capacity: 64 frames).
    let (metrics_tx, _) = tokio::sync::broadcast::channel(64);

    // Generate or load persisted API token when running in remote mode.
    let api_token = if remote {
        let token = load_or_generate_token();
        tracing::info!("[SERVER] Remote mode enabled. API token: {}", token);
        Some(token)
    } else {
        None
    };

    let addr = if remote { "0.0.0.0:8008" } else { "127.0.0.1:8008" };

    // Camera starts as None — opened in background thread after server starts.
    let camera: Arc<std::sync::Mutex<Option<dlc_capture::CameraCapture>>> =
        Arc::new(std::sync::Mutex::new(None));

    let server_state = ServerState {
        app:    Arc::new(RwLock::new(app_state)),
        camera: camera.clone(),
        models: Arc::new(Models {
            detector: std::sync::Mutex::new(detector),
            swapper:  std::sync::Mutex::new(swapper),
            enhancer_gfpgan:  std::sync::Mutex::new(enhancer_gfpgan),
            enhancer_gpen256: std::sync::Mutex::new(enhancer_gpen256),
            enhancer_gpen512: std::sync::Mutex::new(enhancer_gpen512),
            dml_lock: std::sync::Mutex::new(()),
        }),
        metrics_tx,
        gpu_provider: gpu_provider_name,
        remote_mode: remote,
        bind_address: addr.to_string(),
        api_token,
    };

    let app = build_router(server_state, remote);

    tracing::info!("[SERVER] Rust backend starting on {addr}");

    // Open camera in background — server is available immediately.
    // Camera becomes available when OpenCV finishes probing (10-30s on Windows).
    std::thread::spawn(move || {
        tracing::info!("[CAMERA] Opening camera 0 in background...");
        match dlc_capture::CameraCapture::open(0) {
            Ok(cam) => {
                *camera.lock().unwrap() = Some(cam);
                tracing::info!("[CAMERA] Camera 0 ready");
            }
            Err(e) => {
                tracing::warn!("[CAMERA] Camera 0 failed: {e} — test frames will be used");
            }
        }
    });

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

fn parse_npu_flag() -> bool {
    std::env::args().any(|a| a == "--npu")
}

/// Load persisted token from ~/.deep-forge/api-token, or generate + save a new one.
fn load_or_generate_token() -> String {
    let token_dir = dirs_or_fallback();
    let token_path = token_dir.join("api-token");

    // Try loading existing token.
    if let Ok(token) = std::fs::read_to_string(&token_path) {
        let token = token.trim().to_string();
        if token.len() >= 16 {
            tracing::info!("Loaded persisted API token from {}", token_path.display());
            return token;
        }
    }

    // Generate new token and persist it.
    let token = generate_token();
    let _ = std::fs::create_dir_all(&token_dir);
    if let Err(e) = std::fs::write(&token_path, &token) {
        tracing::warn!("Could not persist token to {}: {e}", token_path.display());
    } else {
        tracing::info!("New API token saved to {}", token_path.display());
    }
    token
}

/// Returns ~/.deep-forge/ (or a fallback if home dir is unavailable).
fn dirs_or_fallback() -> std::path::PathBuf {
    if let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
    {
        std::path::PathBuf::from(home).join(".deep-forge")
    } else {
        std::path::PathBuf::from(".deep-forge")
    }
}

fn generate_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    // Combine multiple sources of randomness for a stronger token.
    // Each RandomState::new() seeds from the OS; we hash two independent
    // states together with timing data to produce 128 bits of output.
    let s1 = RandomState::new();
    let s2 = RandomState::new();
    let mut h1 = s1.build_hasher();
    let mut h2 = s2.build_hasher();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    h1.write_u64(nanos);
    h2.write_u64(nanos.wrapping_mul(0x517cc1b727220a95)); // mixing constant
    format!("{:016x}{:016x}", h1.finish(), h2.finish())
}

// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Manager;
use std::sync::Mutex;
use std::process::{Child, Command};
use sysinfo::System;

// Hold sidecar child so it gets killed when the app exits
struct SidecarChild(Mutex<Option<Child>>);

struct MetricsState(Mutex<System>);

#[derive(serde::Serialize)]
struct SystemMetrics {
    cpu_percent: f32,
    ram_used_gb: f64,
    ram_total_gb: f64,
}

#[tauri::command]
fn get_backend_url() -> String {
    "http://localhost:8008".to_string()
}

#[tauri::command]
fn get_system_metrics(state: tauri::State<MetricsState>) -> SystemMetrics {
    let mut sys = state.0.lock().unwrap();
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    SystemMetrics {
        cpu_percent: sys.global_cpu_usage(),
        ram_used_gb: sys.used_memory() as f64 / 1_073_741_824.0,
        ram_total_gb: sys.total_memory() as f64 / 1_073_741_824.0,
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![get_backend_url, get_system_metrics])
        .setup(|app| {
            app.manage(MetricsState(Mutex::new(System::new_all())));

            let resource_dir = app.path().resource_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            let models_dir = resource_dir.join("models");

            // Resolve sidecar binary next to the main app exe.
            let server_exe = resolve_server_exe(&resource_dir);

            println!("[TAURI] resource_dir: {}", resource_dir.display());
            println!("[TAURI] server_exe: {}", server_exe.display());
            println!("[TAURI] models_dir: {}", models_dir.display());

            let child = Command::new(&server_exe)
                .args(["--models-dir", &models_dir.to_string_lossy()])
                .spawn()
                .unwrap_or_else(|e| {
                    panic!("failed to spawn sidecar at {}: {e}", server_exe.display());
                });

            app.manage(SidecarChild(Mutex::new(Some(child))));
            println!("[TAURI] Backend sidecar started");
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(state) = window.try_state::<SidecarChild>() {
                    if let Ok(mut guard) = state.0.lock() {
                        if let Some(mut child) = guard.take() {
                            let _ = child.kill();
                            println!("[TAURI] Backend sidecar stopped");
                        }
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Find the server exe. Checks multiple candidate paths:
/// 1. resource_dir/deep-live-cam-server.exe  (NSIS flat install)
/// 2. resource_dir/binaries/deep-live-cam-server-{triple}.exe  (Tauri sidecar convention)
/// 3. Same directory as the app exe
fn resolve_server_exe(resource_dir: &std::path::Path) -> std::path::PathBuf {
    let triple = if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc"
    } else if cfg!(target_os = "linux") {
        "x86_64-unknown-linux-gnu"
    } else {
        "aarch64-apple-darwin"
    };

    let candidates = [
        resource_dir.join(format!("deep-live-cam-server{}", std::env::consts::EXE_SUFFIX)),
        resource_dir.join(format!("binaries/deep-live-cam-server-{triple}{}", std::env::consts::EXE_SUFFIX)),
    ];

    for path in &candidates {
        if path.exists() {
            return path.clone();
        }
    }

    // Fallback: try next to the current exe
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let next_to_exe = dir.join(format!("deep-live-cam-server{}", std::env::consts::EXE_SUFFIX));
            if next_to_exe.exists() {
                return next_to_exe;
            }
        }
    }

    // Last resort — return the first candidate path and let spawn() produce the error
    candidates[0].clone()
}

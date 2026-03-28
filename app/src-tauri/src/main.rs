// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Manager;
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::CommandChild;
use std::sync::Mutex;

// Hold sidecar child so it gets killed when the app exits
struct SidecarChild(Mutex<Option<CommandChild>>);

#[tauri::command]
fn get_backend_url() -> String {
    "http://localhost:8008".to_string()
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![get_backend_url])
        .setup(|app| {
            // Resolve models directory relative to the sidecar binary
            let resource_dir = app.path().resource_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            let models_dir = resource_dir.join("models");

            let sidecar = app.shell()
                .sidecar("binaries/deep-live-cam-server")
                .expect("failed to create sidecar command")
                .args(["--models-dir", &models_dir.to_string_lossy()]);

            let (_rx, child) = sidecar.spawn()
                .expect("failed to spawn sidecar");

            app.manage(SidecarChild(Mutex::new(Some(child))));

            println!("[TAURI] Backend sidecar started (models: {})", models_dir.display());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(state) = window.try_state::<SidecarChild>() {
                    if let Ok(mut guard) = state.0.lock() {
                        if let Some(child) = guard.take() {
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

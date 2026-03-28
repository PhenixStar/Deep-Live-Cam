// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri_plugin_shell::ShellExt;

#[tauri::command]
fn get_backend_url() -> String {
    "http://localhost:8008".to_string()
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![get_backend_url])
        .setup(|app| {
            // Spawn the Python sidecar when the app starts
            let sidecar = app.shell()
                .sidecar("binaries/deep-live-cam-server")
                .expect("failed to create sidecar command");

            let (mut _rx, _child) = sidecar.spawn()
                .expect("failed to spawn sidecar");

            println!("[TAURI] Python backend sidecar started");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

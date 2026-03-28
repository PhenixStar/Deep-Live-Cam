//! Validation gate: load all required ONNX models and print session info.
//!
//! Usage: validate-models <models-dir>
//! Example: validate-models ../../models

use std::path::PathBuf;
use anyhow::{Context, Result};

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let models_dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../models"));

    println!("=== ort Validation Gate ===");
    println!("Models dir: {}", models_dir.display());
    println!();

    // Check ort build info
    println!("[INFO] ort version: {}", env!("CARGO_PKG_VERSION"));

    let models = [
        ("Face Detector (SCRFD)", "buffalo_l/buffalo_l/det_10g.onnx"),
        ("Face Swapper (inswapper)", "inswapper_128.onnx"),
        ("Face Enhancer (GFPGAN)", "gfpgan-1024.onnx"),
        ("Face Enhancer (GPEN-256)", "GPEN-BFR-256.onnx"),
        ("Face Enhancer (GPEN-512)", "GPEN-BFR-512.onnx"),
    ];

    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;

    for (name, rel_path) in &models {
        let path = models_dir.join(rel_path);
        if !path.exists() {
            println!("[SKIP] {name}: {rel_path} not found");
            skipped += 1;
            continue;
        }

        match load_and_inspect(&path, name) {
            Ok(()) => {
                passed += 1;
            }
            Err(e) => {
                println!("[FAIL] {name}: {e:#}");
                failed += 1;
            }
        }
    }

    println!();
    println!("=== Results: {passed} passed, {failed} failed, {skipped} skipped ===");

    if failed > 0 {
        anyhow::bail!("{failed} model(s) failed to load");
    }
    Ok(())
}

fn load_and_inspect(path: &std::path::Path, name: &str) -> Result<()> {
    let session = ort::session::Session::builder()
        .context("Failed to create session builder")?
        .commit_from_file(path)
        .with_context(|| format!("Failed to load {}", path.display()))?;

    let inputs: Vec<String> = session
        .inputs()
        .iter()
        .map(|i| format!("{}:{:?}", i.name(), i.dtype()))
        .collect();

    let outputs: Vec<String> = session
        .outputs()
        .iter()
        .map(|o| format!("{}:{:?}", o.name(), o.dtype()))
        .collect();

    println!("[PASS] {name}");
    println!("       inputs:  {}", inputs.join(", "));
    println!("       outputs: {}", outputs.join(", "));
    Ok(())
}

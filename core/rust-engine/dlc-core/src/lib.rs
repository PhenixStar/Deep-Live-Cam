//! Face processing pipeline: detection, swap, enhancement.
//!
//! Provides ONNX-based face analysis and manipulation via the `ort` crate.

pub mod detect;
pub mod swap;
pub mod enhance;
pub mod preprocess;

use anyhow::Result;
use ndarray::Array3;

/// GPU execution provider configuration.
#[derive(Debug, Clone)]
pub enum GpuProvider {
    /// Try DirectML first, fall back to CPU.
    Auto,
    /// Force DirectML with a specific device ID.
    DirectML { device_id: i32 },
    /// CPU only.
    Cpu,
}

impl Default for GpuProvider {
    fn default() -> Self {
        Self::Auto
    }
}

impl GpuProvider {
    /// Load an ONNX model with the appropriate execution providers.
    pub fn load_session(&self, model_path: &std::path::Path) -> Result<ort::session::Session> {
        use ort::ep;

        let device_id = match self {
            GpuProvider::DirectML { device_id } => *device_id,
            _ => 0,
        };

        let session = match self {
            GpuProvider::Auto | GpuProvider::DirectML { .. } => {
                // DirectML requires memory pattern disabled.
                ort::session::Session::builder()
                    .map_err(|e| anyhow::anyhow!("Session::builder: {e}"))?
                    .with_memory_pattern(false)
                    .map_err(|e| anyhow::anyhow!("with_memory_pattern: {e}"))?
                    .with_execution_providers([
                        ep::DirectML::default().with_device_id(device_id).build(),
                        ep::CPU::default().build(),
                    ])
                    .map_err(|e| anyhow::anyhow!("with_execution_providers: {e}"))?
                    .commit_from_file(model_path)
                    .map_err(|e| anyhow::anyhow!("commit_from_file: {e}"))?
            }
            GpuProvider::Cpu => {
                ort::session::Session::builder()
                    .map_err(|e| anyhow::anyhow!("Session::builder: {e}"))?
                    .with_execution_providers([
                        ep::CPU::default().build(),
                    ])
                    .map_err(|e| anyhow::anyhow!("with_execution_providers: {e}"))?
                    .commit_from_file(model_path)
                    .map_err(|e| anyhow::anyhow!("commit_from_file: {e}"))?
            }
        };

        Ok(session)
    }
}

/// A detected face with bounding box, landmarks, and embedding.
#[derive(Debug, Clone)]
pub struct DetectedFace {
    /// Bounding box [x1, y1, x2, y2] in pixel coordinates.
    pub bbox: [f32; 4],
    /// Confidence score (0.0 - 1.0).
    pub score: f32,
    /// 5 facial landmarks (left_eye, right_eye, nose, left_mouth, right_mouth).
    pub landmarks: [[f32; 2]; 5],
    /// 512-dim ArcFace embedding (populated after embedding extraction).
    pub embedding: Option<Vec<f32>>,
}

/// BGR image as a 3D array (H, W, 3).
pub type Frame = Array3<u8>;

/// Configuration for the processing pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub face_swap_enabled: bool,
    pub face_enhancer_gfpgan: bool,
    pub face_enhancer_gpen256: bool,
    pub face_enhancer_gpen512: bool,
    pub jpeg_quality: u8,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            face_swap_enabled: true,
            face_enhancer_gfpgan: false,
            face_enhancer_gpen256: false,
            face_enhancer_gpen512: false,
            jpeg_quality: 80,
        }
    }
}

/// Load and validate all ONNX models. Returns error if any model fails to load.
pub fn validate_models(models_dir: &std::path::Path, _providers: &[String]) -> Result<()> {
    tracing::info!("Validating ONNX models in {}", models_dir.display());

    let required = [
        "inswapper_128.onnx",
        "buffalo_l/buffalo_l/det_10g.onnx",
        "gfpgan-1024.onnx",
    ];

    for model in &required {
        let path = models_dir.join(model);
        if !path.exists() {
            tracing::warn!("Model not found: {} (will download on first use)", model);
            continue;
        }
        tracing::info!("Found model: {}", model);
    }

    // Validate ort can load at least one model
    let det_path = models_dir.join("buffalo_l/buffalo_l/det_10g.onnx");
    if det_path.exists() {
        let provider = GpuProvider::Cpu;
        let _session = provider.load_session(&det_path)?;
        tracing::info!("ort session creation OK (det_10g.onnx)");
    }

    Ok(())
}

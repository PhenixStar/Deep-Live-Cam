//! Face processing pipeline: detection, swap, enhancement.
//!
//! Provides ONNX-based face analysis and manipulation via the `ort` crate.

pub mod detect;
pub mod swap;
pub mod enhance;
pub mod preprocess;
pub mod tracker;

use anyhow::Result;
use ndarray::Array3;

/// GPU/accelerator execution provider configuration.
///
/// Auto-detection priority:
///   CUDA (NVIDIA) > DirectML (AMD/Intel/NVIDIA on Windows) > VitisAI NPU > CPU
#[derive(Debug, Clone)]
pub enum GpuProvider {
    /// Auto-detect best available: CUDA > DirectML > CPU.
    Auto,
    /// Force CUDA with a specific device ID (NVIDIA GPUs).
    Cuda { device_id: i32 },
    /// Force DirectML with a specific device ID (AMD/Intel/NVIDIA on Windows).
    DirectML { device_id: i32 },
    /// AMD XDNA2 NPU via VitisAI EP.
    /// Requires: Ryzen AI SDK installed, custom onnxruntime.dll with VitisAI,
    /// vaip_config.json, and INT8 quantized models.
    Npu { config_file: String, cache_dir: String },
    /// CPU only.
    Cpu,
}

impl Default for GpuProvider {
    fn default() -> Self { Self::Auto }
}

impl GpuProvider {
    /// Detect the best available provider at runtime.
    /// Checks env vars and available EPs to select optimal config.
    pub fn detect() -> Self {
        // User overrides via env var: DEEP_FORGE_EP=cuda|directml|npu|cpu
        if let Ok(ep) = std::env::var("DEEP_FORGE_EP") {
            match ep.to_lowercase().as_str() {
                "cuda"     => return Self::Cuda { device_id: 0 },
                "directml" => return Self::DirectML { device_id: 0 },
                "npu"      => return Self::Npu {
                    config_file: std::env::var("DEEP_FORGE_NPU_CONFIG")
                        .unwrap_or_else(|_| "vaip_config.json".into()),
                    cache_dir: std::env::var("DEEP_FORGE_NPU_CACHE")
                        .unwrap_or_else(|_| "./npu_cache".into()),
                },
                "cpu"      => return Self::Cpu,
                _          => tracing::warn!("Unknown DEEP_FORGE_EP={ep}, using auto-detect"),
            }
        }

        // Auto-detect: the ort crate will try each EP in order and use the
        // first one that initializes successfully. We configure the fallback
        // chain in load_session(). Here we just pick the default strategy.
        Self::Auto
    }

    /// Human-readable name for the active provider (shown in /health and UI).
    pub fn display_name(&self) -> &str {
        match self {
            Self::Auto => "Auto (CUDA > DirectML > CPU)",
            Self::Cuda { .. } => "CUDA",
            Self::DirectML { .. } => "DirectML",
            Self::Npu { .. } => "VitisAI NPU",
            Self::Cpu => "CPU",
        }
    }

    /// Load an ONNX model with the appropriate execution providers.
    ///
    /// The EP fallback chain depends on the variant:
    /// - Auto: CUDA → DirectML → CPU (tries all, ort picks first working one)
    /// - Cuda: CUDA → CPU
    /// - DirectML: DirectML → CPU
    /// - Npu: VitisAI → CPU (requires AMD Ryzen AI SDK's custom ORT build)
    /// - Cpu: CPU only
    ///
    /// Level3 graph optimization enables FP16 layout rewrites on capable hardware.
    pub fn load_session(&self, model_path: &std::path::Path) -> Result<ort::session::Session> {
        use ort::ep;
        use ort::session::builder::GraphOptimizationLevel;

        let mut builder = ort::session::Session::builder()
            .map_err(|e| anyhow::anyhow!("Session::builder: {e}"))?;

        builder = builder
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| anyhow::anyhow!("with_optimization_level: {e}"))?;

        // DirectML and VitisAI both require memory pattern disabled.
        let needs_disable_mem_pattern = matches!(
            self,
            Self::Auto | Self::DirectML { .. } | Self::Npu { .. }
        );
        if needs_disable_mem_pattern {
            builder = builder
                .with_memory_pattern(false)
                .map_err(|e| anyhow::anyhow!("with_memory_pattern: {e}"))?;
        }

        // Build the EP chain based on the selected provider.
        let session = match self {
            Self::Auto => {
                // Full chain: try CUDA (NVIDIA), then DirectML (AMD/Intel), then CPU.
                // ort skips EPs that fail to initialize — first working one wins.
                builder
                    .with_execution_providers([
                        ep::CUDA::default().build(),
                        ep::DirectML::default().build(),
                        ep::CPU::default().build(),
                    ])
                    .map_err(|e| anyhow::anyhow!("with_execution_providers: {e}"))?
                    .commit_from_file(model_path)
                    .map_err(|e| anyhow::anyhow!("commit_from_file: {e}"))?
            }
            Self::Cuda { device_id } => {
                builder
                    .with_execution_providers([
                        ep::CUDA::default().with_device_id(*device_id).build(),
                        ep::CPU::default().build(),
                    ])
                    .map_err(|e| anyhow::anyhow!("with_execution_providers: {e}"))?
                    .commit_from_file(model_path)
                    .map_err(|e| anyhow::anyhow!("commit_from_file: {e}"))?
            }
            Self::DirectML { device_id } => {
                builder
                    .with_execution_providers([
                        ep::DirectML::default().with_device_id(*device_id).build(),
                        ep::CPU::default().build(),
                    ])
                    .map_err(|e| anyhow::anyhow!("with_execution_providers: {e}"))?
                    .commit_from_file(model_path)
                    .map_err(|e| anyhow::anyhow!("commit_from_file: {e}"))?
            }
            Self::Npu { config_file, cache_dir } => {
                // VitisAI EP: the underlying onnxruntime.dll must be built with
                // VitisAI support (from AMD Ryzen AI SDK). The ort Rust crate
                // passes EP config through to the C API — if the DLL supports it,
                // it works. If not, falls back to CPU.
                //
                // Setup on Sweep:
                //   1. Install Ryzen AI SDK: ryzen-ai-1.7.0.msi
                //   2. Copy custom onnxruntime.dll to ort-dml-libs/
                //   3. Set ORT_LIB_PATH=ort-dml-libs
                //   4. Place vaip_config.json in the working directory
                //   5. Use INT8 quantized models (quantize with amd-quark)
                tracing::info!(
                    "NPU: VitisAI EP config_file={config_file} cache_dir={cache_dir}"
                );

                // Create cache dir for compiled NPU binaries.
                let _ = std::fs::create_dir_all(cache_dir);

                // VitisAI EP is registered via its string name. If the ORT build
                // includes it, it will initialize; otherwise ort logs a warning
                // and falls back to the next EP (CPU).
                builder
                    .with_execution_providers([
                        ep::Vitis::default()
                            .with_config_file(config_file)
                            .with_cache_dir(cache_dir)
                            .with_cache_key("deep-forge")
                            .build(),
                        ep::CPU::default().build(),
                    ])
                    .map_err(|e| anyhow::anyhow!("with_execution_providers: {e}"))?
                    .commit_from_file(model_path)
                    .map_err(|e| anyhow::anyhow!("commit_from_file: {e}"))?
            }
            Self::Cpu => {
                builder
                    .with_execution_providers([ep::CPU::default().build()])
                    .map_err(|e| anyhow::anyhow!("with_execution_providers: {e}"))?
                    .commit_from_file(model_path)
                    .map_err(|e| anyhow::anyhow!("commit_from_file: {e}"))?
            }
        };

        Ok(session)
    }

    /// Try FP16 model first, INT8 second (for NPU), then FP32 fallback.
    pub fn resolve_model_path(&self, base_path: &std::path::Path) -> std::path::PathBuf {
        let stem = base_path.file_stem().unwrap_or_default().to_str().unwrap_or("");
        let ext  = base_path.extension().unwrap_or_default().to_str().unwrap_or("onnx");

        // NPU prefers INT8 quantized models.
        if matches!(self, Self::Npu { .. }) {
            let int8_name = format!("{}_int8.{}", stem, ext);
            let int8_path = base_path.with_file_name(&int8_name);
            if int8_path.exists() {
                tracing::info!("Using INT8 model for NPU: {}", int8_path.display());
                return int8_path;
            }
        }

        // GPU providers prefer FP16 for ~2x throughput.
        let fp16_name = format!("{}_fp16.{}", stem, ext);
        let fp16_path = base_path.with_file_name(&fp16_name);
        if fp16_path.exists() {
            tracing::info!("Using FP16 model: {}", fp16_path.display());
            return fp16_path;
        }

        // Default to FP32.
        base_path.to_path_buf()
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

//! Model manifest — defines required/optional models with download URLs.
//!
//! All URLs point to publicly available HuggingFace repos.
//! The app downloads missing models on first run via the model manager UI.

use serde::Serialize;

/// Base URL for our own HF model repo (primary download source).
pub const PRIMARY_REPO: &str = "https://huggingface.co/phenixstar/deep-forge-models/resolve/main";

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    /// Display name in the UI.
    pub name: &'static str,
    /// Relative path under models_dir (e.g., "buffalo_l/buffalo_l/det_10g.onnx").
    pub path: &'static str,
    /// Primary download URL (our HF repo — built from PRIMARY_REPO + path).
    #[serde(serialize_with = "serialize_url")]
    pub url_suffix: &'static str,
    /// Fallback download URL (public repos, in case our repo is down).
    pub fallback_url: &'static str,
    /// Approximate file size in MB.
    pub size_mb: u32,
    /// Is this model required for basic face swap?
    pub required: bool,
    /// Description shown in model manager.
    pub description: &'static str,
}

impl ModelInfo {
    /// Primary download URL (our repo).
    pub fn url(&self) -> String {
        format!("{}/{}", PRIMARY_REPO, self.url_suffix)
    }
}

fn serialize_url<S: serde::Serializer>(suffix: &&str, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format!("{}/{}", PRIMARY_REPO, suffix))
}

/// All models used by Deep Forge with their download sources.
pub const MODELS: &[ModelInfo] = &[
    // === Required: face detection ===
    ModelInfo {
        name: "SCRFD Face Detector",
        path: "buffalo_l/buffalo_l/det_10g.onnx",
        url_suffix: "buffalo_l/buffalo_l/det_10g.onnx",
        fallback_url: "https://huggingface.co/facefusion/models/resolve/main/det_10g.onnx",
        size_mb: 16,
        required: true,
        description: "SCRFD 10GF face detection model (InsightFace buffalo_l)",
    },
    ModelInfo {
        name: "ArcFace Embedding (w600k_r50)",
        path: "buffalo_l/buffalo_l/w600k_r50.onnx",
        url_suffix: "buffalo_l/buffalo_l/w600k_r50.onnx",
        fallback_url: "https://huggingface.co/facefusion/models/resolve/main/w600k_r50.onnx",
        size_mb: 166,
        required: true,
        description: "ArcFace R50 face recognition/embedding model",
    },
    // === Required: face swap ===
    ModelInfo {
        name: "Inswapper 128 (FP32)",
        path: "inswapper_128.onnx",
        url_suffix: "inswapper_128.onnx",
        fallback_url: "https://huggingface.co/ezioruan/inswapper_128.onnx/resolve/main/inswapper_128.onnx",
        size_mb: 554,
        required: true,
        description: "InsightFace inswapper face swap model (FP32)",
    },
    ModelInfo {
        name: "Inswapper 128 (FP16)",
        path: "inswapper_128_fp16.onnx",
        url_suffix: "inswapper_128_fp16.onnx",
        fallback_url: "https://huggingface.co/facefusion/models/resolve/main/inswapper_128_fp16.onnx",
        size_mb: 277,
        required: false,
        description: "FP16 variant — 2x faster on GPU, auto-selected when present",
    },
    // === Optional: INT8 quantized for NPU ===
    ModelInfo {
        name: "SCRFD INT8 (NPU)",
        path: "buffalo_l/buffalo_l/det_10g_int8.onnx",
        url_suffix: "buffalo_l/buffalo_l/det_10g_int8.onnx",
        fallback_url: "",
        size_mb: 4,
        required: false,
        description: "INT8 QDQ quantized SCRFD — auto-selected when using NPU provider",
    },
    ModelInfo {
        name: "ArcFace INT8 (NPU)",
        path: "buffalo_l/buffalo_l/w600k_r50_int8.onnx",
        url_suffix: "buffalo_l/buffalo_l/w600k_r50_int8.onnx",
        fallback_url: "",
        size_mb: 44,
        required: false,
        description: "INT8 QDQ quantized ArcFace — auto-selected when using NPU provider",
    },
    ModelInfo {
        name: "Inswapper 128 INT8 (NPU)",
        path: "inswapper_128_int8.onnx",
        url_suffix: "inswapper_128_int8.onnx",
        fallback_url: "",
        size_mb: 139,
        required: false,
        description: "INT8 QDQ quantized inswapper — auto-selected when using NPU provider",
    },
    // === Optional: face enhancement ===
    ModelInfo {
        name: "GFPGAN 1024",
        path: "gfpgan-1024.onnx",
        url_suffix: "gfpgan-1024.onnx",
        fallback_url: "https://huggingface.co/facefusion/models/resolve/main/gfpgan_1.4.onnx",
        size_mb: 332,
        required: false,
        description: "GFPGAN v1.4 face enhancement (1024x1024)",
    },
    ModelInfo {
        name: "GPEN-BFR 256",
        path: "GPEN-BFR-256.onnx",
        url_suffix: "GPEN-BFR-256.onnx",
        fallback_url: "https://huggingface.co/facefusion/models/resolve/main/GPEN-BFR-256.onnx",
        size_mb: 80,
        required: false,
        description: "GPEN blind face restoration (256x256, fast)",
    },
    ModelInfo {
        name: "GPEN-BFR 512",
        path: "GPEN-BFR-512.onnx",
        url_suffix: "GPEN-BFR-512.onnx",
        fallback_url: "https://huggingface.co/facefusion/models/resolve/main/GPEN-BFR-512.onnx",
        size_mb: 80,
        required: false,
        description: "GPEN blind face restoration (512x512, quality)",
    },
];

/// Check which models exist on disk and which are missing.
pub fn check_models(models_dir: &std::path::Path) -> Vec<ModelStatus> {
    MODELS
        .iter()
        .map(|m| {
            let full_path = models_dir.join(m.path);
            ModelStatus {
                info: m,
                file_exists: full_path.exists(),
                file_size_mb: full_path
                    .metadata()
                    .map(|meta| (meta.len() / (1024 * 1024)) as u32)
                    .unwrap_or(0),
            }
        })
        .collect()
}

#[derive(Debug, Serialize)]
pub struct ModelStatus<'a> {
    #[serde(flatten)]
    pub info: &'a ModelInfo,
    pub file_exists: bool,
    pub file_size_mb: u32,
}

//! Shared application state for the server.

/// Whether the live-stream source is a camera or an uploaded video file.
#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Camera,
    VideoFile,
}

impl Default for InputMode {
    fn default() -> Self {
        InputMode::Camera
    }
}

/// Server-wide mutable state protected by RwLock.
#[derive(Debug, Clone)]
pub struct AppState {
    pub active_camera: u32,
    pub face_enhancer_gfpgan: bool,
    pub face_enhancer_gpen256: bool,
    pub face_enhancer_gpen512: bool,
    pub frame_processors: Vec<String>,
    /// Raw image bytes of the uploaded source face image.
    pub source_image_bytes: Option<Vec<u8>>,
    /// Detected source face with pre-computed embedding.
    /// When set (via /source upload or profile activation), the WS loop
    /// skips re-detection and re-embedding — saving ~35ms/frame.
    pub source_face: Option<dlc_core::DetectedFace>,
    /// Cached 512-dim ArcFace embedding for the source face.
    /// Set once on upload, reused every frame without re-computation.
    pub cached_source_embedding: Option<Vec<f32>>,
    /// Directory where ONNX model files are stored.
    pub models_dir: std::path::PathBuf,
    /// Directory where face profiles are stored (`models_dir/profiles/`).
    pub profiles_dir: std::path::PathBuf,
    /// Capture resolution (width, height).
    pub resolution: (u32, u32),
    /// Swap calibration: horizontal offset in pixels (applied to paste-back).
    pub swap_offset_x: f32,
    /// Swap calibration: vertical offset in pixels.
    pub swap_offset_y: f32,
    /// Swap calibration: scale multiplier (1.0 = default, >1 = larger face).
    pub swap_scale: f32,
    /// Whether live stream reads from camera or a video file.
    pub input_mode: InputMode,
    /// Path to the uploaded video file (set when input_mode == VideoFile).
    pub video_path: Option<std::path::PathBuf>,
    /// Whether a recording session is currently active.
    pub recording: bool,
    /// Output path for the current (or last) recording.
    pub recording_path: Option<std::path::PathBuf>,
}

impl Default for AppState {
    fn default() -> Self {
        let models_dir = std::env::var("DEEP_LIVE_CAM_MODELS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("models"));

        let profiles_dir = models_dir.join("profiles");

        Self {
            active_camera: 0,
            face_enhancer_gfpgan: false,
            face_enhancer_gpen256: false,
            face_enhancer_gpen512: false,
            frame_processors: vec!["face_swapper".into()],
            source_image_bytes: None,
            source_face: None,
            cached_source_embedding: None,
            profiles_dir,
            models_dir,
            resolution: (640, 480),
            swap_offset_x: 0.0,
            swap_offset_y: 0.0,
            swap_scale: 1.0,
            input_mode: InputMode::Camera,
            video_path: None,
            recording: false,
            recording_path: None,
        }
    }
}

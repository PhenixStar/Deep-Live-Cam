//! Face profile CRUD: create, read, update, delete profiles with multi-photo
//! averaged embeddings for source face selection.
//!
//! Storage layout per profile:
//! ```text
//! {profiles_dir}/{uuid}/
//!     meta.json        — serialised ProfileMeta
//!     embedding.bin    — 512 x f32 LE bytes (2048 B)
//!     thumbnail.jpg    — 128x128 best-aligned face crop
//!     photo_0.jpg … photo_5.jpg
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use axum::{
    extract::{Json, Multipart, Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::router::{decode_to_bgr_frame, ServerState};
use dlc_core::Frame;

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

const MAX_PHOTOS: usize = 6;
const EMBEDDING_DIM: usize = 512;
const THUMBNAIL_SIZE: u32 = 128;

/// Persisted metadata (stored as `meta.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileMeta {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Filenames of uploaded photos (e.g. ["photo_0.jpg", "photo_1.jpg"]).
    pub photos: Vec<String>,
    /// Average detection confidence across photos with detected faces.
    pub score: f32,
    /// Unix timestamp (seconds) when the profile was created.
    pub created: u64,
}

/// JSON returned to clients (list endpoint).
#[derive(Serialize)]
pub struct ProfileSummary {
    pub id: Uuid,
    pub name: String,
    pub photo_count: usize,
    pub score: f32,
    /// Unix timestamp (seconds) when the profile was created.
    pub created: u64,
    /// Base64-encoded JPEG thumbnail (empty string if none).
    pub thumbnail_b64: String,
}

/// Per-photo metadata returned to the frontend.
#[derive(Serialize, Clone)]
pub struct PhotoSlot {
    /// Filename (e.g. "photo_0.jpg") used as a stable identifier.
    pub url: String,
    /// Detection confidence for this photo (0.0 if not yet computed).
    pub score: f32,
    /// Whether a face was detected in this photo.
    pub has_face: bool,
}

/// JSON returned to clients (detail endpoint).
#[derive(Serialize)]
pub struct ProfileDetail {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub photos: Vec<PhotoSlot>,
    pub has_embedding: bool,
    pub score: f32,
    pub created: u64,
    pub thumbnail_b64: String,
}

/// Body for POST / PUT.
#[derive(Deserialize)]
pub struct ProfileBody {
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

fn profile_dir(profiles_dir: &Path, id: Uuid) -> PathBuf {
    profiles_dir.join(id.to_string())
}

fn read_meta(dir: &Path) -> Result<ProfileMeta> {
    let data = std::fs::read(dir.join("meta.json")).context("read meta.json")?;
    serde_json::from_slice(&data).context("parse meta.json")
}

fn write_meta(dir: &Path, meta: &ProfileMeta) -> Result<()> {
    let data = serde_json::to_vec_pretty(meta).context("serialise meta")?;
    std::fs::write(dir.join("meta.json"), data).context("write meta.json")
}

fn read_embedding(dir: &Path) -> Option<Vec<f32>> {
    let bytes = std::fs::read(dir.join("embedding.bin")).ok()?;
    if bytes.len() != EMBEDDING_DIM * 4 {
        return None;
    }
    Some(
        bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
    )
}

fn write_embedding(dir: &Path, emb: &[f32]) -> Result<()> {
    let bytes: Vec<u8> = emb.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(dir.join("embedding.bin"), bytes).context("write embedding.bin")
}

fn read_thumbnail_b64(dir: &Path) -> String {
    match std::fs::read(dir.join("thumbnail.jpg")) {
        Ok(bytes) => {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        }
        Err(_) => String::new(),
    }
}

/// Build the list of photo metadata objects expected by the frontend.
/// Uses the profile's average score as a proxy (per-photo scores are not
/// persisted separately). `has_face` is true whenever the photo file exists
/// and the profile-level score > 0 (meaning at least one face was found
/// during the last recompute pass).
fn build_photo_slots(dir: &Path, meta: &ProfileMeta) -> Vec<PhotoSlot> {
    meta.photos
        .iter()
        .map(|name| {
            let exists = dir.join(name).exists();
            PhotoSlot {
                url: name.clone(),
                score: if exists && meta.score > 0.0 { meta.score } else { 0.0 },
                has_face: exists && meta.score > 0.0,
            }
        })
        .collect()
}

/// Assemble a full `ProfileDetail` response from meta + directory.
fn build_profile_detail(dir: &Path, meta: &ProfileMeta) -> ProfileDetail {
    ProfileDetail {
        id: meta.id,
        name: meta.name.clone(),
        description: meta.description.clone(),
        photos: build_photo_slots(dir, meta),
        has_embedding: dir.join("embedding.bin").exists(),
        score: meta.score,
        created: meta.created,
        thumbnail_b64: read_thumbnail_b64(dir),
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Crop and resize a face region to 128x128 JPEG bytes for the thumbnail.
fn make_thumbnail(frame: &Frame, bbox: &[f32; 4]) -> Result<Vec<u8>> {
    use image::codecs::jpeg::JpegEncoder;
    use image::{ImageBuffer, ImageEncoder, Rgb};

    let (fh, fw, _) = frame.dim();
    let x1 = (bbox[0].max(0.0) as usize).min(fw.saturating_sub(1));
    let y1 = (bbox[1].max(0.0) as usize).min(fh.saturating_sub(1));
    let x2 = (bbox[2].max(0.0).ceil() as usize).min(fw);
    let y2 = (bbox[3].max(0.0).ceil() as usize).min(fh);
    let cw = x2.saturating_sub(x1).max(1);
    let ch = y2.saturating_sub(y1).max(1);

    // Extract crop as RGB.
    let mut rgb_buf: Vec<u8> = Vec::with_capacity(ch * cw * 3);
    for y in y1..y2 {
        for x in x1..x2 {
            rgb_buf.push(frame[[y, x, 2]]); // R
            rgb_buf.push(frame[[y, x, 1]]); // G
            rgb_buf.push(frame[[y, x, 0]]); // B
        }
    }
    let crop: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_raw(cw as u32, ch as u32, rgb_buf)
            .context("thumbnail crop")?;

    let resized = image::imageops::resize(
        &crop,
        THUMBNAIL_SIZE,
        THUMBNAIL_SIZE,
        image::imageops::FilterType::Triangle,
    );

    let mut out: Vec<u8> = Vec::new();
    let enc = JpegEncoder::new_with_quality(&mut out, 85);
    enc.write_image(
        resized.as_raw(),
        THUMBNAIL_SIZE,
        THUMBNAIL_SIZE,
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Embedding computation
// ---------------------------------------------------------------------------

/// For each photo file in `dir`, detect the face, extract embedding, and
/// return (embeddings, scores, best_bbox_index).
fn extract_all_embeddings(
    dir: &Path,
    photos: &[String],
    state: &ServerState,
) -> Result<(Vec<Vec<f32>>, Vec<f32>, Option<usize>)> {
    let mut det_guard = state.models.detector.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
    let detector = det_guard
        .as_mut()
        .context("detector model not loaded")?;

    let mut swap_guard = state.models.swapper.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
    let swapper = swap_guard
        .as_mut()
        .context("swapper model not loaded")?;

    let mut embeddings: Vec<Vec<f32>> = Vec::new();
    let mut scores: Vec<f32> = Vec::new();
    let mut best_idx: Option<usize> = None;
    let mut best_score: f32 = 0.0;

    for (i, photo_name) in photos.iter().enumerate() {
        let photo_path = dir.join(photo_name);
        let bytes = match std::fs::read(&photo_path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let frame = match decode_to_bgr_frame(&bytes) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let faces = match detector.detect(&frame, 0.3) {
            Ok(f) => f,
            Err(_) => continue,
        };
        if let Some(face) = faces.first() {
            if let Ok(emb) = swapper.get_embedding(&frame, face) {
                embeddings.push(emb);
                scores.push(face.score);
                if face.score > best_score {
                    best_score = face.score;
                    best_idx = Some(i);
                }
            }
        }
    }

    Ok((embeddings, scores, best_idx))
}

fn average_and_normalize(embeddings: &[Vec<f32>]) -> Option<Vec<f32>> {
    if embeddings.is_empty() {
        return None;
    }
    let n = embeddings.len() as f32;
    let dim = embeddings[0].len();
    let mut avg = vec![0.0f32; dim];
    for emb in embeddings {
        for (i, v) in emb.iter().enumerate() {
            avg[i] += v;
        }
    }
    for v in avg.iter_mut() {
        *v /= n;
    }
    // L2 normalize
    let norm = avg.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-10);
    for v in avg.iter_mut() {
        *v /= norm;
    }
    Some(avg)
}

/// Recompute embedding + thumbnail for a profile. Call after photo add/remove.
fn recompute_profile(dir: &Path, meta: &mut ProfileMeta, state: &ServerState) -> Result<()> {
    if meta.photos.is_empty() {
        // No photos: clear embedding and thumbnail.
        let _ = std::fs::remove_file(dir.join("embedding.bin"));
        let _ = std::fs::remove_file(dir.join("thumbnail.jpg"));
        meta.score = 0.0;
        return Ok(());
    }

    let (embeddings, scores, best_idx) = extract_all_embeddings(dir, &meta.photos, state)?;

    if let Some(avg) = average_and_normalize(&embeddings) {
        write_embedding(dir, &avg)?;
    } else {
        let _ = std::fs::remove_file(dir.join("embedding.bin"));
    }

    // Average score across photos that had a detected face.
    meta.score = if scores.is_empty() {
        0.0
    } else {
        scores.iter().sum::<f32>() / scores.len() as f32
    };

    // Generate thumbnail from the best-scoring photo.
    if let Some(idx) = best_idx {
        let photo_path = dir.join(&meta.photos[idx]);
        if let Ok(bytes) = std::fs::read(&photo_path) {
            if let Ok(frame) = decode_to_bgr_frame(&bytes) {
                let mut det_guard = state.models.detector.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
                if let Some(detector) = det_guard.as_mut() {
                    if let Ok(faces) = detector.detect(&frame, 0.3) {
                        if let Some(face) = faces.first() {
                            if let Ok(thumb_bytes) = make_thumbnail(&frame, &face.bbox) {
                                let _ = std::fs::write(dir.join("thumbnail.jpg"), thumb_bytes);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Error helper
// ---------------------------------------------------------------------------

fn err_json(status: StatusCode, msg: impl ToString) -> Response {
    (status, Json(serde_json::json!({"error": msg.to_string()}))).into_response()
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /profiles — list all profiles.
pub async fn list_profiles(State(state): State<ServerState>) -> Response {
    let profiles_dir = {
        let app = state.app.read().unwrap();
        app.profiles_dir.clone()
    };

    if !profiles_dir.exists() {
        return Json(serde_json::json!([])).into_response();
    }

    let entries = match std::fs::read_dir(&profiles_dir) {
        Ok(e) => e,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, format!("read profiles dir: {e}")),
    };

    let mut profiles: Vec<ProfileSummary> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Ok(meta) = read_meta(&path) {
            profiles.push(ProfileSummary {
                id: meta.id,
                name: meta.name,
                photo_count: meta.photos.len(),
                score: meta.score,
                created: meta.created,
                thumbnail_b64: read_thumbnail_b64(&path),
            });
        }
    }

    // Sort by creation time, newest first.
    profiles.sort_by(|a, b| b.created.cmp(&a.created));

    Json(profiles).into_response()
}

/// POST /profiles — create a new profile.
pub async fn create_profile(
    State(state): State<ServerState>,
    Json(body): Json<ProfileBody>,
) -> Response {
    let name = match body.name {
        Some(n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => return err_json(StatusCode::BAD_REQUEST, "name is required"),
    };

    let profiles_dir = {
        let app = state.app.read().unwrap();
        app.profiles_dir.clone()
    };

    let id = Uuid::new_v4();
    let dir = profile_dir(&profiles_dir, id);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, format!("mkdir: {e}"));
    }

    let meta = ProfileMeta {
        id,
        name,
        description: body.description.unwrap_or_default(),
        photos: vec![],
        score: 0.0,
        created: now_unix(),
    };

    if let Err(e) = write_meta(&dir, &meta) {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, format!("write meta: {e}"));
    }

    (StatusCode::CREATED, Json(serde_json::json!({
        "id": meta.id,
        "name": meta.name,
        "description": meta.description,
        "created": meta.created
    })))
        .into_response()
}

/// GET /profiles/{id}
pub async fn get_profile(
    State(state): State<ServerState>,
    AxumPath(id): AxumPath<Uuid>,
) -> Response {
    let profiles_dir = {
        let app = state.app.read().unwrap();
        app.profiles_dir.clone()
    };
    let dir = profile_dir(&profiles_dir, id);

    let meta = match read_meta(&dir) {
        Ok(m) => m,
        Err(_) => return err_json(StatusCode::NOT_FOUND, "profile not found"),
    };

    Json(build_profile_detail(&dir, &meta)).into_response()
}

/// PUT /profiles/{id}
pub async fn update_profile(
    State(state): State<ServerState>,
    AxumPath(id): AxumPath<Uuid>,
    Json(body): Json<ProfileBody>,
) -> Response {
    let profiles_dir = {
        let app = state.app.read().unwrap();
        app.profiles_dir.clone()
    };
    let dir = profile_dir(&profiles_dir, id);

    let mut meta = match read_meta(&dir) {
        Ok(m) => m,
        Err(_) => return err_json(StatusCode::NOT_FOUND, "profile not found"),
    };

    if let Some(n) = body.name {
        if n.trim().is_empty() {
            return err_json(StatusCode::BAD_REQUEST, "name cannot be empty");
        }
        meta.name = n.trim().to_string();
    }
    if let Some(d) = body.description {
        meta.description = d;
    }

    if let Err(e) = write_meta(&dir, &meta) {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, format!("write meta: {e}"));
    }

    Json(serde_json::json!({"status": "ok"})).into_response()
}

/// DELETE /profiles/{id}
pub async fn delete_profile(
    State(state): State<ServerState>,
    AxumPath(id): AxumPath<Uuid>,
) -> Response {
    let profiles_dir = {
        let app = state.app.read().unwrap();
        app.profiles_dir.clone()
    };
    let dir = profile_dir(&profiles_dir, id);

    if !dir.exists() {
        return err_json(StatusCode::NOT_FOUND, "profile not found");
    }

    if let Err(e) = std::fs::remove_dir_all(&dir) {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, format!("delete: {e}"));
    }

    Json(serde_json::json!({"status": "deleted"})).into_response()
}

/// POST /profiles/{id}/photos — upload a photo (multipart, field name "photo").
pub async fn add_photo(
    State(state): State<ServerState>,
    AxumPath(id): AxumPath<Uuid>,
    mut multipart: Multipart,
) -> Response {
    let profiles_dir = {
        let app = state.app.read().unwrap();
        app.profiles_dir.clone()
    };
    let dir = profile_dir(&profiles_dir, id);

    let mut meta = match read_meta(&dir) {
        Ok(m) => m,
        Err(_) => return err_json(StatusCode::NOT_FOUND, "profile not found"),
    };

    if meta.photos.len() >= MAX_PHOTOS {
        return err_json(StatusCode::BAD_REQUEST, format!("max {MAX_PHOTOS} photos"));
    }

    // Read the first multipart field.
    let bytes = match multipart.next_field().await {
        Ok(Some(field)) => match field.bytes().await {
            Ok(b) => b,
            Err(e) => return err_json(StatusCode::BAD_REQUEST, format!("read field: {e}")),
        },
        Ok(None) => return err_json(StatusCode::BAD_REQUEST, "no file uploaded"),
        Err(e) => return err_json(StatusCode::BAD_REQUEST, format!("multipart: {e}")),
    };

    // Validate it is a decodable image.
    if image::load_from_memory(&bytes).is_err() {
        return err_json(StatusCode::UNPROCESSABLE_ENTITY, "invalid image data");
    }

    // Check models are loaded (needed for face detection + embedding).
    {
        let det_loaded = state.models.detector.lock().map(|g| g.is_some()).unwrap_or(false);
        let swap_loaded = state.models.swapper.lock().map(|g| g.is_some()).unwrap_or(false);
        if !det_loaded || !swap_loaded {
            return err_json(
                StatusCode::SERVICE_UNAVAILABLE,
                "models not loaded (detector/swapper required)",
            );
        }
    }

    // Quick-check: detect a face in the uploaded image.
    {
        let frame = match decode_to_bgr_frame(&bytes) {
            Ok(f) => f,
            Err(e) => return err_json(StatusCode::UNPROCESSABLE_ENTITY, format!("decode: {e}")),
        };
        let mut det_guard = state.models.detector.lock().unwrap();
        if let Some(detector) = det_guard.as_mut() {
            match detector.detect(&frame, 0.3) {
                Ok(faces) if faces.is_empty() => {
                    return err_json(StatusCode::UNPROCESSABLE_ENTITY, "no face detected in photo");
                }
                Err(e) => {
                    return err_json(StatusCode::INTERNAL_SERVER_ERROR, format!("detection: {e}"));
                }
                _ => {}
            }
        }
    }

    // Find next available index.
    let idx = (0..MAX_PHOTOS)
        .find(|i| !meta.photos.contains(&format!("photo_{i}.jpg")))
        .unwrap_or(meta.photos.len());
    let filename = format!("photo_{idx}.jpg");

    // Save the photo as JPEG (re-encode to normalize format).
    let img = image::load_from_memory(&bytes).unwrap();
    let rgb = img.to_rgb8();
    if let Err(e) = rgb.save(dir.join(&filename)) {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, format!("save photo: {e}"));
    }

    meta.photos.push(filename.clone());

    // Recompute embedding + thumbnail.
    if let Err(e) = recompute_profile(&dir, &mut meta, &state) {
        tracing::warn!("recompute_profile failed: {e}");
        // Non-fatal: the photo is saved, embedding may be stale.
    }

    if let Err(e) = write_meta(&dir, &meta) {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, format!("write meta: {e}"));
    }

    Json(serde_json::json!({
        "photos": build_photo_slots(&dir, &meta),
        "thumbnail_b64": read_thumbnail_b64(&dir),
    }))
    .into_response()
}

/// DELETE /profiles/{id}/photos/{idx}
pub async fn delete_photo(
    State(state): State<ServerState>,
    AxumPath((id, idx)): AxumPath<(Uuid, usize)>,
) -> Response {
    let profiles_dir = {
        let app = state.app.read().unwrap();
        app.profiles_dir.clone()
    };
    let dir = profile_dir(&profiles_dir, id);

    let mut meta = match read_meta(&dir) {
        Ok(m) => m,
        Err(_) => return err_json(StatusCode::NOT_FOUND, "profile not found"),
    };

    let filename = format!("photo_{idx}.jpg");
    if !meta.photos.contains(&filename) {
        return err_json(StatusCode::NOT_FOUND, format!("photo_{idx}.jpg not found"));
    }

    // Remove file from disk.
    let _ = std::fs::remove_file(dir.join(&filename));
    meta.photos.retain(|p| p != &filename);

    // Recompute embedding + thumbnail.
    if let Err(e) = recompute_profile(&dir, &mut meta, &state) {
        tracing::warn!("recompute_profile after delete failed: {e}");
    }

    if let Err(e) = write_meta(&dir, &meta) {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, format!("write meta: {e}"));
    }

    Json(serde_json::json!({
        "photos": build_photo_slots(&dir, &meta),
        "thumbnail_b64": read_thumbnail_b64(&dir),
    }))
    .into_response()
}

/// POST /profiles/{id}/activate — load cached embedding as active source.
pub async fn activate_profile(
    State(state): State<ServerState>,
    AxumPath(id): AxumPath<Uuid>,
) -> Response {
    let profiles_dir = {
        let app = state.app.read().unwrap();
        app.profiles_dir.clone()
    };
    let dir = profile_dir(&profiles_dir, id);

    let meta = match read_meta(&dir) {
        Ok(m) => m,
        Err(_) => return err_json(StatusCode::NOT_FOUND, "profile not found"),
    };

    let embedding = match read_embedding(&dir) {
        Some(e) => e,
        None => {
            return err_json(
                StatusCode::CONFLICT,
                "profile has no computed embedding (upload photos first)",
            )
        }
    };

    // Build a synthetic DetectedFace with the cached embedding so the swap
    // pipeline can use it as the source identity.
    let source_face = dlc_core::DetectedFace {
        bbox: [0.0, 0.0, 0.0, 0.0],
        score: meta.score,
        landmarks: [[0.0; 2]; 5],
        embedding: Some(embedding),
    };

    // Also load the best photo bytes as source_image_bytes so that the
    // WS pipeline can re-detect if needed (e.g., for landmark-based alignment).
    let source_bytes = meta
        .photos
        .first()
        .and_then(|p| std::fs::read(dir.join(p)).ok());

    {
        let mut app = state.app.write().unwrap();
        app.source_face = Some(source_face);
        if let Some(bytes) = source_bytes {
            app.source_image_bytes = Some(bytes);
        }
    }

    tracing::info!(profile_id = %id, name = %meta.name, "profile activated");

    Json(serde_json::json!({
        "status": "activated",
        "profile_id": id,
        "name": meta.name,
        "score": meta.score
    }))
    .into_response()
}

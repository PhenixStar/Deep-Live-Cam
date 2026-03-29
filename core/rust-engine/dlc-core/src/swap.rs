//! Face swap pipeline: ArcFace embedding extraction + inswapper_128 inference.
//!
//! # Pipeline
//! 1. `get_embedding`: align face to 112x112 → ArcFace ONNX → L2-normalize → Vec<f32>
//! 2. `swap`: align target to 128x128 → inswapper ONNX (source embedding + target) →
//!    denormalize → inverse-warp result back into frame

use std::path::Path;

use anyhow::{Context, Result};
use ort::{session::Session, value::Tensor};

use crate::{DetectedFace, Frame};
use crate::preprocess::{align_face_arcface, align_face_swap, affine_matrix_swap, invert_affine};

// ---------------------------------------------------------------------------
// FaceSwapper
// ---------------------------------------------------------------------------

/// Pre-loaded ONNX sessions for ArcFace embedding extraction and inswapper_128.
pub struct FaceSwapper {
    arcface_session: Session,
    swap_session: Session,
}

impl FaceSwapper {
    /// Load both ONNX models from `models_dir`.
    ///
    /// Expected paths:
    /// - `<models_dir>/buffalo_l/buffalo_l/w600k_r50.onnx`  (ArcFace R50)
    /// - `<models_dir>/inswapper_128.onnx`                  (inswapper)
    pub fn new(models_dir: &Path, provider: &crate::GpuProvider) -> Result<Self> {
        let arcface_path = models_dir.join("buffalo_l/buffalo_l/w600k_r50.onnx");
        let swap_path = models_dir.join("inswapper_128.onnx");

        tracing::info!("Loading ArcFace from {}", arcface_path.display());
        let arcface_session = provider.load_session(&arcface_path)
            .with_context(|| format!("ArcFace: failed to load {}", arcface_path.display()))?;

        tracing::info!("Loading inswapper from {}", swap_path.display());
        let swap_session = provider.load_session(&swap_path)
            .with_context(|| format!("inswapper: failed to load {}", swap_path.display()))?;

        Ok(Self { arcface_session, swap_session })
    }

    /// Align `face` in `frame` and run ArcFace to produce a 512-dim L2-normalised
    /// embedding vector.
    pub fn get_embedding(&mut self, frame: &Frame, face: &DetectedFace) -> Result<Vec<f32>> {
        // 1. Align to 112x112 using the ArcFace canonical template.
        let aligned = align_face_arcface(frame, &face.landmarks)
            .context("get_embedding: face alignment failed")?;

        // 2. BGR HWC u8 → RGB NCHW f32, normalised: pixel/127.5 - 1.0
        let (shape, data) = bgr_hwc_to_rgb_nchw_normalized(&aligned, 112, 112)?;

        // 3. Build input tensor using (shape, data) tuple — avoids ndarray version clash.
        let tensor = Tensor::<f32>::from_array((shape, data.into_boxed_slice()))
            .context("get_embedding: failed to create input tensor")?;

        // 4. Run ArcFace session.  `ort::inputs![value]` returns an array directly.
        let outputs = self.arcface_session
            .run(ort::inputs![tensor])
            .context("get_embedding: ArcFace inference failed")?;

        // 5. Extract output via try_extract_tensor (returns (&Shape, &[f32])).
        let (_shape, raw) = outputs[0]
            .try_extract_tensor::<f32>()
            .context("get_embedding: failed to extract output")?;

        anyhow::ensure!(raw.len() == 512,
            "ArcFace output has {} elements, expected 512", raw.len());

        // 6. L2-normalise.
        let norm = l2_norm(raw);
        let embedding = raw.iter().map(|v| v / norm).collect::<Vec<_>>();

        Ok(embedding)
    }

    /// Swap the source face identity onto the target face region within `frame`.
    ///
    /// `source_face.embedding` must be populated before calling this (use
    /// `get_embedding`).  Modifies `frame` in-place.
    pub fn swap(
        &mut self,
        source_face: &DetectedFace,
        target_face: &DetectedFace,
        frame: &mut Frame,
    ) -> Result<()> {
        let src_emb = source_face
            .embedding
            .as_ref()
            .context("swap: source_face.embedding is None — call get_embedding first")?;

        anyhow::ensure!(src_emb.len() == 512,
            "swap: source embedding has {} dims, expected 512", src_emb.len());

        // 1. Align target face to 128x128.
        let aligned_target = align_face_swap(frame, &target_face.landmarks)
            .context("swap: target face alignment failed")?;

        // 2. "target" input tensor [1,3,128,128], normalised 0-1, RGB.
        let (tgt_shape, tgt_data) = bgr_hwc_to_rgb_nchw_01(&aligned_target, 128, 128)?;
        let target_tensor = Tensor::<f32>::from_array((tgt_shape, tgt_data.into_boxed_slice()))
            .context("swap: failed to create target tensor")?;

        // 3. "source" input tensor [1,512].
        let src_shape = vec![1i64, 512i64];
        let src_data = src_emb.clone().into_boxed_slice();
        let source_tensor = Tensor::<f32>::from_array((src_shape, src_data))
            .context("swap: failed to create source tensor")?;

        // 4. Run inswapper with named inputs.
        let outputs = self.swap_session
            .run(ort::inputs! {
                "target" => target_tensor,
                "source" => source_tensor
            })
            .context("swap: inswapper inference failed")?;

        // 5. Extract output [1,3,128,128] f32.
        let (_out_shape, out_raw) = outputs[0]
            .try_extract_tensor::<f32>()
            .context("swap: failed to extract swapper output")?;

        anyhow::ensure!(out_raw.len() == 3 * 128 * 128,
            "swap: unexpected output size {}", out_raw.len());

        // 6. Denormalize RGB NCHW [0,1] → BGR HWC u8.
        let swapped_bgr = rgb_nchw_01_to_bgr_hwc(out_raw, 128, 128);

        // 7. Inverse-warp patch back into frame using the forward affine matrix.
        let fwd_mat = affine_matrix_swap(&target_face.landmarks);
        paste_back(frame, &swapped_bgr, &fwd_mat)
            .context("swap: paste_back failed")?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tensor preparation helpers (raw Vec, no ndarray — avoids version mismatch)
// ---------------------------------------------------------------------------

/// BGR HWC u8 → RGB NCHW f32, `pixel / 127.5 - 1.0`.  Shape: `[1,3,h,w]`.
fn bgr_hwc_to_rgb_nchw_normalized(
    img: &Frame,
    h: usize,
    w: usize,
) -> Result<(Vec<i64>, Vec<f32>)> {
    anyhow::ensure!(img.shape() == [h, w, 3], "Expected [{h},{w},3] frame");
    let mut data = vec![0f32; 3 * h * w];
    let hw = h * w;
    for y in 0..h {
        for x in 0..w {
            // Frame is BGR; swap to RGB for the model.
            let b = img[[y, x, 0]] as f32;
            let g = img[[y, x, 1]] as f32;
            let r = img[[y, x, 2]] as f32;
            data[0 * hw + y * w + x] = r / 127.5 - 1.0;
            data[1 * hw + y * w + x] = g / 127.5 - 1.0;
            data[2 * hw + y * w + x] = b / 127.5 - 1.0;
        }
    }
    Ok((vec![1i64, 3, h as i64, w as i64], data))
}

/// BGR HWC u8 → RGB NCHW f32, `pixel / 255.0`.  Shape: `[1,3,h,w]`.
fn bgr_hwc_to_rgb_nchw_01(
    img: &Frame,
    h: usize,
    w: usize,
) -> Result<(Vec<i64>, Vec<f32>)> {
    anyhow::ensure!(img.shape() == [h, w, 3], "Expected [{h},{w},3] frame");
    let mut data = vec![0f32; 3 * h * w];
    let hw = h * w;
    for y in 0..h {
        for x in 0..w {
            let b = img[[y, x, 0]] as f32 / 255.0;
            let g = img[[y, x, 1]] as f32 / 255.0;
            let r = img[[y, x, 2]] as f32 / 255.0;
            data[0 * hw + y * w + x] = r;
            data[1 * hw + y * w + x] = g;
            data[2 * hw + y * w + x] = b;
        }
    }
    Ok((vec![1i64, 3, h as i64, w as i64], data))
}

/// RGB NCHW f32 `[0,1]` slice → BGR HWC u8 `Frame`.
fn rgb_nchw_01_to_bgr_hwc(data: &[f32], h: usize, w: usize) -> Frame {
    let hw = h * w;
    let mut out = ndarray::Array3::<u8>::zeros((h, w, 3));
    for y in 0..h {
        for x in 0..w {
            let r = (data[0 * hw + y * w + x].clamp(0.0, 1.0) * 255.0).round() as u8;
            let g = (data[1 * hw + y * w + x].clamp(0.0, 1.0) * 255.0).round() as u8;
            let b = (data[2 * hw + y * w + x].clamp(0.0, 1.0) * 255.0).round() as u8;
            out[[y, x, 0]] = b; // store as BGR
            out[[y, x, 1]] = g;
            out[[y, x, 2]] = r;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Paste-back: inverse-warp 128x128 patch into frame
// ---------------------------------------------------------------------------

/// Elliptical face mask for the 128x128 patch.
/// Returns alpha in [0,1] — 1.0 at center, smooth falloff at edges.
/// Matches InsightFace's blending approach.
fn face_mask_128() -> Vec<f32> {
    let size = 128usize;
    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    // Ellipse radii — slightly smaller than the patch to feather edges.
    let rx = size as f32 * 0.42; // horizontal
    let ry = size as f32 * 0.45; // vertical (taller for face shape)
    let feather = 0.15; // feather zone as fraction of radius

    let mut mask = vec![0.0f32; size * size];
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 - cx) / rx;
            let dy = (y as f32 - cy) / ry;
            let dist = (dx * dx + dy * dy).sqrt();
            let alpha = if dist <= 1.0 - feather {
                1.0
            } else if dist <= 1.0 + feather {
                // Smooth cosine falloff in the feather zone
                let t = (dist - (1.0 - feather)) / (2.0 * feather);
                0.5 * (1.0 + (std::f32::consts::PI * t).cos())
            } else {
                0.0
            };
            mask[y * size + x] = alpha;
        }
    }
    mask
}

/// Pre-computed face mask (computed once, reused).
fn get_face_mask() -> &'static [f32] {
    use std::sync::OnceLock;
    static MASK: OnceLock<Vec<f32>> = OnceLock::new();
    MASK.get_or_init(face_mask_128)
}

/// For each pixel in `frame`, compute where it maps in the 128x128 `swapped`
/// patch using the FORWARD matrix (frame→patch), then alpha-blend using an
/// elliptical face mask for seamless edges.
fn paste_back(frame: &mut Frame, swapped: &Frame, fwd_mat: &[f32; 6]) -> Result<()> {
    let (fh, fw) = (frame.shape()[0], frame.shape()[1]);
    let (ph, pw) = (128usize, 128usize);
    let mask = get_face_mask();

    // fwd_mat maps frame coords → patch coords directly.
    // No inversion needed — we iterate frame pixels and use fwd_mat to find patch coords.
    let (ia, ib, itx, ic, id, ity) = (fwd_mat[0], fwd_mat[1], fwd_mat[2],
                                       fwd_mat[3], fwd_mat[4], fwd_mat[5]);

    // Compute ROI: transform 128x128 patch corners to frame coords via inv_mat.
    let inv = invert_affine(fwd_mat)?;
    let corners = [(0.0f32, 0.0f32), (pw as f32, 0.0), (0.0, ph as f32), (pw as f32, ph as f32)];
    let (mut min_x, mut min_y) = (fw as f32, fh as f32);
    let (mut max_x, mut max_y) = (0.0f32, 0.0f32);
    for (cx, cy) in &corners {
        let fx = inv[0] * cx + inv[1] * cy + inv[2];
        let fy = inv[3] * cx + inv[4] * cy + inv[5];
        min_x = min_x.min(fx); min_y = min_y.min(fy);
        max_x = max_x.max(fx); max_y = max_y.max(fy);
    }
    let y_start = (min_y.floor() as usize).max(0).min(fh);
    let y_end   = (max_y.ceil() as usize + 1).min(fh);
    let x_start = (min_x.floor() as usize).max(0).min(fw);
    let x_end   = (max_x.ceil() as usize + 1).min(fw);

    for fy in y_start..y_end {
        for fx in x_start..x_end {
            let fx_f = fx as f32;
            let fy_f = fy as f32;

            let px = ia * fx_f + ib * fy_f + itx;
            let py = ic * fx_f + id * fy_f + ity;

            if px < 0.0 || py < 0.0 || px >= pw as f32 || py >= ph as f32 {
                continue;
            }

            // Elliptical mask alpha at this patch position.
            let mx = (px as usize).min(pw - 1);
            let my = (py as usize).min(ph - 1);
            let alpha = mask[my * pw + mx];
            if alpha < 0.001 { continue; }

            let x0 = px.floor() as usize;
            let y0 = py.floor() as usize;
            let x1 = (x0 + 1).min(pw - 1);
            let y1 = (y0 + 1).min(ph - 1);
            let wx = px - x0 as f32;
            let wy = py - y0 as f32;

            for c in 0..3usize {
                let p00 = swapped[[y0, x0, c]] as f32;
                let p01 = swapped[[y0, x1, c]] as f32;
                let p10 = swapped[[y1, x0, c]] as f32;
                let p11 = swapped[[y1, x1, c]] as f32;
                let swapped_val = p00 * (1.0 - wx) * (1.0 - wy)
                    + p01 * wx * (1.0 - wy)
                    + p10 * (1.0 - wx) * wy
                    + p11 * wx * wy;
                // Alpha-blend: result = swapped * alpha + original * (1 - alpha)
                let orig = frame[[fy, fx, c]] as f32;
                frame[[fy, fx, c]] = (swapped_val * alpha + orig * (1.0 - alpha))
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Math helpers
// ---------------------------------------------------------------------------

fn l2_norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-10)
}

//! Face enhancement (GFPGAN, GPEN) via ONNX Runtime.
//!
//! Implements bbox-based crop → model inference → alpha-blend paste-back.
//! Normalization: BGR→RGB, [0,255] → [-1,1] (FFHQ convention used by GFPGAN).

use crate::Frame;
use anyhow::{Context, Result};
use ndarray::Array3;

// ── image helpers (pure ndarray, no external resize crate) ───────────────────

/// Bilinear-resize a `Frame` (HWC u8) to `(out_h, out_w)`.
fn resize_frame(src: &Frame, out_h: usize, out_w: usize) -> Frame {
    let (src_h, src_w, _) = src.dim();
    let mut dst = Array3::<u8>::zeros((out_h, out_w, 3));

    for dy in 0..out_h {
        let sy_f = (dy as f32 + 0.5) * src_h as f32 / out_h as f32 - 0.5;
        let sy0 = (sy_f.floor() as isize).clamp(0, src_h as isize - 1) as usize;
        let sy1 = (sy0 + 1).min(src_h - 1);
        let ty = sy_f - sy_f.floor();

        for dx in 0..out_w {
            let sx_f = (dx as f32 + 0.5) * src_w as f32 / out_w as f32 - 0.5;
            let sx0 = (sx_f.floor() as isize).clamp(0, src_w as isize - 1) as usize;
            let sx1 = (sx0 + 1).min(src_w - 1);
            let tx = sx_f - sx_f.floor();

            for c in 0..3 {
                let v00 = src[[sy0, sx0, c]] as f32;
                let v01 = src[[sy0, sx1, c]] as f32;
                let v10 = src[[sy1, sx0, c]] as f32;
                let v11 = src[[sy1, sx1, c]] as f32;
                let v = v00 * (1.0 - tx) * (1.0 - ty)
                    + v01 * tx * (1.0 - ty)
                    + v10 * (1.0 - tx) * ty
                    + v11 * tx * ty;
                dst[[dy, dx, c]] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    dst
}

/// Crop `frame[y0..y1, x0..x1, ..]` into an owned Frame.
fn crop_frame(frame: &Frame, y0: usize, x0: usize, y1: usize, x1: usize) -> Frame {
    use ndarray::s;
    frame.slice(s![y0..y1, x0..x1, ..]).to_owned()
}

// ── tensor pre/post-processing ────────────────────────────────────────────────

/// Convert HWC BGR u8 to flat NCHW RGB f32 in [-1,1].
///
/// Returns `(shape, data)` ready for `ort::value::Tensor::from_array`.
fn preprocess(face: &Frame, input_size: usize) -> (Vec<i64>, Vec<f32>) {
    let resized = resize_frame(face, input_size, input_size);

    // NCHW: [1, 3, H, W]
    let mut data = vec![0.0f32; 3 * input_size * input_size];
    for row in 0..input_size {
        for col in 0..input_size {
            // Frame is HWC BGR; model expects RGB.
            let b = resized[[row, col, 0]] as f32;
            let g = resized[[row, col, 1]] as f32;
            let r = resized[[row, col, 2]] as f32;

            let pixel_base = row * input_size + col;
            // channel 0 = R
            data[pixel_base] = (r / 255.0 - 0.5) / 0.5;
            // channel 1 = G
            data[input_size * input_size + pixel_base] = (g / 255.0 - 0.5) / 0.5;
            // channel 2 = B
            data[2 * input_size * input_size + pixel_base] = (b / 255.0 - 0.5) / 0.5;
        }
    }

    let shape = vec![1_i64, 3, input_size as i64, input_size as i64];
    (shape, data)
}

/// Convert flat NCHW RGB f32 in [-1,1] (shape [1,3,H,W]) to HWC BGR u8.
fn postprocess(data: &[f32], output_size: usize) -> Frame {
    let plane = output_size * output_size;
    let mut face = Array3::<u8>::zeros((output_size, output_size, 3));
    for row in 0..output_size {
        for col in 0..output_size {
            let idx = row * output_size + col;
            // output channels: 0=R, 1=G, 2=B
            let r = ((data[idx] + 1.0) / 2.0 * 255.0).clamp(0.0, 255.0) as u8;
            let g = ((data[plane + idx] + 1.0) / 2.0 * 255.0).clamp(0.0, 255.0) as u8;
            let b = ((data[2 * plane + idx] + 1.0) / 2.0 * 255.0).clamp(0.0, 255.0) as u8;
            // Store as BGR to match Frame convention.
            face[[row, col, 0]] = b;
            face[[row, col, 1]] = g;
            face[[row, col, 2]] = r;
        }
    }
    face
}

// ── paste-back ────────────────────────────────────────────────────────────────

/// Alpha-blend `enhanced` back into the crop region `[y0..y1, x0..x1]` of `frame`.
///
/// A feathered mask (5 % linear ramp on each edge) avoids hard seams.
fn paste_back(
    frame: &mut Frame,
    enhanced: &Frame,
    y0: usize,
    x0: usize,
    y1: usize,
    x1: usize,
) {
    let crop_h = y1 - y0;
    let crop_w = x1 - x0;

    // Resize enhanced to match crop dimensions (model output == input_size).
    let resized = resize_frame(enhanced, crop_h, crop_w);

    let border_h = ((crop_h as f32 * 0.05) as usize).max(1);
    let border_w = ((crop_w as f32 * 0.05) as usize).max(1);

    for dy in 0..crop_h {
        let fy = if dy < border_h {
            dy as f32 / border_h as f32
        } else if dy >= crop_h - border_h {
            (crop_h - 1 - dy) as f32 / border_h as f32
        } else {
            1.0_f32
        };

        for dx in 0..crop_w {
            let fx = if dx < border_w {
                dx as f32 / border_w as f32
            } else if dx >= crop_w - border_w {
                (crop_w - 1 - dx) as f32 / border_w as f32
            } else {
                1.0_f32
            };

            let alpha = fy * fx;
            let fy_idx = y0 + dy;
            let fx_idx = x0 + dx;

            for c in 0..3 {
                let orig = frame[[fy_idx, fx_idx, c]] as f32;
                let enh = resized[[dy, dx, c]] as f32;
                frame[[fy_idx, fx_idx, c]] =
                    (orig * (1.0 - alpha) + enh * alpha).clamp(0.0, 255.0) as u8;
            }
        }
    }
}

// ── public API ────────────────────────────────────────────────────────────────

/// Face enhancer using GFPGAN or GPEN ONNX models.
///
/// Supported models and their `input_size`:
/// - `gfpgan-1024.onnx`   → 1024
/// - `GPEN-BFR-512.onnx`  → 512
/// - `GPEN-BFR-256.onnx`  → 256
pub struct FaceEnhancer {
    session: ort::session::Session,
    /// Square spatial dimension the model expects (256, 512, or 1024).
    input_size: u32,
}

impl FaceEnhancer {
    /// Load an ONNX enhancement model from `model_path`.
    ///
    /// `input_size` must match the model's spatial dimension (256, 512, or 1024).
    pub fn new(model_path: &std::path::Path, input_size: u32) -> Result<Self> {
        let session = ort::session::Session::builder()
            .context("Failed to create ort session builder")?
            .commit_from_file(model_path)
            .with_context(|| format!("Failed to load model: {}", model_path.display()))?;

        tracing::info!(
            model = %model_path.display(),
            input_size,
            "FaceEnhancer loaded"
        );

        Ok(Self { session, input_size })
    }

    /// Enhance the face region defined by `bbox` (`[x1, y1, x2, y2]`) in-place.
    ///
    /// A 10 % padding is added around the bbox so the model sees surrounding
    /// context. Coordinates are clamped to frame bounds.
    pub fn enhance(&mut self, frame: &mut Frame, bbox: &[f32; 4]) -> Result<()> {
        let (frame_h, frame_w, _) = frame.dim();

        // ── 1. Compute padded crop bounds ─────────────────────────────────────
        let bw = (bbox[2] - bbox[0]).max(1.0);
        let bh = (bbox[3] - bbox[1]).max(1.0);
        let pad_x = bw * 0.10;
        let pad_y = bh * 0.10;

        let x0 = ((bbox[0] - pad_x).floor() as isize).clamp(0, frame_w as isize - 1) as usize;
        let y0 = ((bbox[1] - pad_y).floor() as isize).clamp(0, frame_h as isize - 1) as usize;
        let x1 = ((bbox[2] + pad_x).ceil() as isize).clamp(1, frame_w as isize) as usize;
        let y1 = ((bbox[3] + pad_y).ceil() as isize).clamp(1, frame_h as isize) as usize;

        if x1 <= x0 || y1 <= y0 {
            anyhow::bail!("Degenerate crop after padding: [{x0},{y0}]→[{x1},{y1}]");
        }

        // ── 2. Crop ───────────────────────────────────────────────────────────
        let crop = crop_frame(frame, y0, x0, y1, x1);

        // ── 3. Pre-process: HWC BGR u8 → (shape, NCHW RGB f32 [-1,1]) ─────────
        let input_size = self.input_size as usize;
        let (shape, data) = preprocess(&crop, input_size);

        // ── 4. Build ort input tensor ─────────────────────────────────────────
        // Use (Vec<i64>, Vec<f32>) form to avoid ndarray version conflicts.
        let input_tensor = ort::value::Tensor::<f32>::from_array((shape, data))
            .context("Failed to create input tensor")?;

        // ── 5. Run inference ──────────────────────────────────────────────────
        let outputs = self
            .session
            .run(ort::inputs![input_tensor])
            .context("ONNX inference failed")?;

        // ── 6. Extract output: (&Shape, &[f32]) ──────────────────────────────
        let (out_shape, out_data) = outputs[0]
            .try_extract_tensor::<f32>()
            .context("Failed to extract output tensor")?;

        // Determine actual output spatial size from shape [1, 3, H, W].
        let output_size = if out_shape.len() >= 4 {
            out_shape[2] as usize
        } else {
            input_size
        };

        // ── 7. Post-process: NCHW RGB f32 → HWC BGR u8 ───────────────────────
        let enhanced = postprocess(out_data, output_size);

        // ── 8. Alpha-blend back into frame ────────────────────────────────────
        paste_back(frame, &enhanced, y0, x0, y1, x1);

        Ok(())
    }
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array3;

    fn solid_frame(h: usize, w: usize, val: u8) -> Frame {
        Array3::from_elem((h, w, 3), val)
    }

    #[test]
    fn resize_identity() {
        let src = solid_frame(4, 4, 128);
        let out = resize_frame(&src, 4, 4);
        assert_eq!(out, src);
    }

    #[test]
    fn resize_upscale() {
        let src = solid_frame(2, 2, 200);
        let out = resize_frame(&src, 4, 4);
        assert_eq!(out.dim(), (4, 4, 3));
        // All pixels should remain close to 200 for a solid color.
        for v in out.iter() {
            assert!(*v >= 195 && *v <= 205);
        }
    }

    #[test]
    fn crop_correct_dims() {
        let src = solid_frame(100, 100, 50);
        let crop = crop_frame(&src, 10, 20, 40, 70);
        assert_eq!(crop.dim(), (30, 50, 3));
    }

    #[test]
    fn preprocess_shape() {
        let face = solid_frame(8, 8, 128);
        let (shape, data) = preprocess(&face, 8);
        assert_eq!(shape, vec![1, 3, 8, 8]);
        assert_eq!(data.len(), 3 * 8 * 8);
        // 128/255 normalised to [-1,1]: (0.502 - 0.5)/0.5 ≈ 0.004
        for &v in &data {
            assert!(v.abs() < 0.1, "expected near-zero for mid-grey, got {v}");
        }
    }

    #[test]
    fn postprocess_roundtrip() {
        // Black (BGR 0,0,0) → preprocess → postprocess should stay near 0.
        let face = solid_frame(4, 4, 0);
        let (_, data) = preprocess(&face, 4);
        let out = postprocess(&data, 4);
        // All channels should be 0 (or very close due to rounding).
        for &v in out.iter() {
            assert!(v <= 1, "expected near-black, got {v}");
        }
    }

    #[test]
    fn paste_back_full_alpha_replaces() {
        // Enhanced frame filled with 200; original filled with 50.
        // With alpha=1 (center pixels, large enough frame), result should be 200.
        let mut frame = solid_frame(100, 100, 50);
        let enhanced = solid_frame(50, 50, 200); // pre-sized to crop region
        // Crop region is center 50×50, resize_frame will be called inside paste_back.
        paste_back(&mut frame, &enhanced, 25, 25, 75, 75);
        // Center pixel (50,50) should be ≈200.
        let center = frame[[50, 50, 0]];
        assert!(center > 180, "center pixel should be near 200, got {center}");
    }

    #[test]
    fn paste_back_border_blends() {
        // Border pixels should be blended (not pure 200).
        let mut frame = solid_frame(100, 100, 50);
        let enhanced = solid_frame(50, 50, 200);
        paste_back(&mut frame, &enhanced, 25, 25, 75, 75);

        // The very edge pixel (25,25) has alpha=0 (dy=0,dx=0 → fy*fx=0), so it
        // stays at the original value. Check that it was NOT fully replaced.
        let edge = frame[[25, 25, 0]];
        assert_eq!(edge, 50, "edge pixel should remain original, got {edge}");

        // A pixel one step inside the feather zone should be partially blended:
        // it should be between the original (50) and enhanced (200) values.
        let inside = frame[[26, 26, 0]];
        assert!(inside > 50, "pixel inside border should have some enhancement, got {inside}");
        assert!(inside < 200, "pixel inside border should be blended, got {inside}");
    }
}

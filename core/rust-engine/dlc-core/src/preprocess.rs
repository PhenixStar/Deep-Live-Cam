//! Image preprocessing utilities: resize, normalize, affine transforms.

use anyhow::{Context, Result};

/// SCRFD input resolution.
pub const DET_SIZE: usize = 640;

/// Resize and normalize a BGR frame for SCRFD detection input.
///
/// Steps:
/// 1. Letterbox-resize to 640x640 (preserve aspect ratio, pad with the
///    normalised value of a zero pixel: `(0 - 127.5) / 128.0 ≈ -0.996`).
/// 2. Convert u8 BGR → f32, normalize: `(v - 127.5) / 128.0`.
/// 3. Transpose HWC → CHW, add batch dim.
///
/// Returns `(shape, data)` compatible with `ort::value::Tensor::from_array`:
/// - `shape`: `[1i64, 3, 640, 640]`
/// - `data`:  length `1 * 3 * 640 * 640` in CHW order
pub fn preprocess_detection(frame: &crate::Frame) -> Result<(Vec<i64>, Vec<f32>)> {
    let (src_h, src_w, _) = frame.dim();

    // Scale that fits the frame into DET_SIZE × DET_SIZE (top-left placement).
    let scale = (DET_SIZE as f32 / src_h as f32).min(DET_SIZE as f32 / src_w as f32);
    let new_h = (src_h as f32 * scale).round() as usize;
    let new_w = (src_w as f32 * scale).round() as usize;

    tracing::debug!(
        src_h, src_w, new_h, new_w, scale,
        "preprocess_detection: letterbox resize"
    );

    // Normalised value for black (zero-pixel) padding.
    let pad_val = (0.0f32 - 127.5) / 128.0;

    // Allocate HWC buffer, initialised to pad_val for padding regions.
    let mut hwc = vec![pad_val; DET_SIZE * DET_SIZE * 3];

    // Nearest-neighbour resize into top-left of the canvas.
    // Frame is BGR; model expects RGB, so swap B and R channels.
    for dy in 0..new_h {
        let sy = ((dy as f32 / scale) as usize).min(src_h - 1);
        for dx in 0..new_w {
            let sx = ((dx as f32 / scale) as usize).min(src_w - 1);
            let dst = (dy * DET_SIZE + dx) * 3;
            hwc[dst]     = (frame[[sy, sx, 2]] as f32 - 127.5) / 128.0; // R
            hwc[dst + 1] = (frame[[sy, sx, 1]] as f32 - 127.5) / 128.0; // G
            hwc[dst + 2] = (frame[[sy, sx, 0]] as f32 - 127.5) / 128.0; // B
        }
    }

    // HWC → CHW (move channel axis to front).
    let mut chw = vec![0.0f32; 3 * DET_SIZE * DET_SIZE];
    for c in 0..3usize {
        for y in 0..DET_SIZE {
            for x in 0..DET_SIZE {
                chw[c * DET_SIZE * DET_SIZE + y * DET_SIZE + x] =
                    hwc[(y * DET_SIZE + x) * 3 + c];
            }
        }
    }

    let shape = vec![1i64, 3, DET_SIZE as i64, DET_SIZE as i64];
    tracing::debug!("preprocess_detection: elements={}", chw.len());
    Ok((shape, chw))
}

/// Return the scale and top-left padding offsets used by `preprocess_detection`
/// so post-processing can map letterbox detections back to source coordinates.
///
/// Content is placed at top-left (no centering), so pad offsets are always 0.
pub fn letterbox_params(src_h: usize, src_w: usize) -> (f32, usize, usize) {
    let scale = (DET_SIZE as f32 / src_h as f32).min(DET_SIZE as f32 / src_w as f32);
    // pad_top and pad_left are 0 because content starts at (0,0).
    (scale, 0, 0)
}

// ---------------------------------------------------------------------------
// Affine alignment helpers
// ---------------------------------------------------------------------------

/// ArcFace canonical 5-point template for 112x112 output.
const ARCFACE_DST: [[f32; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
];

/// inswapper canonical 5-point template for 128x128 output.
/// Scaled proportionally from the 112x112 ArcFace template (factor ≈ 128/112).
const SWAP_DST: [[f32; 2]; 5] = [
    [43.7644, 58.9670],
    [84.0364, 58.8588],
    [64.0288, 81.9847],
    [47.4849, 105.5606],
    [80.8341, 105.3761],
];

/// Compute the 2x3 similarity-transform matrix (scale + rotation + translation)
/// that maps `src` landmark points to `dst` reference points (Umeyama method).
///
/// Returns coefficients `[a, -b, tx, b, a, ty]` row-major, i.e.
/// `x' = a*x - b*y + tx`, `y' = b*x + a*y + ty`.
pub(crate) fn similarity_transform(src: &[[f32; 2]; 5], dst: &[[f32; 2]; 5]) -> [f32; 6] {
    let n = 5usize;
    let nf = n as f32;

    let mut src_mean = [0f32; 2];
    let mut dst_mean = [0f32; 2];
    for i in 0..n {
        src_mean[0] += src[i][0];
        src_mean[1] += src[i][1];
        dst_mean[0] += dst[i][0];
        dst_mean[1] += dst[i][1];
    }
    src_mean[0] /= nf;
    src_mean[1] /= nf;
    dst_mean[0] /= nf;
    dst_mean[1] /= nf;

    // Variance of src points.
    let mut src_var = 0f32;
    for i in 0..n {
        let dx = src[i][0] - src_mean[0];
        let dy = src[i][1] - src_mean[1];
        src_var += dx * dx + dy * dy;
    }
    src_var /= nf;

    // Cross-covariance H = (1/N) Σ (dst_i - dst_mean)^T (src_i - src_mean)
    let mut h = [[0f32; 2]; 2];
    for i in 0..n {
        let sx = src[i][0] - src_mean[0];
        let sy = src[i][1] - src_mean[1];
        let dx = dst[i][0] - dst_mean[0];
        let dy = dst[i][1] - dst_mean[1];
        h[0][0] += dx * sx;
        h[0][1] += dx * sy;
        h[1][0] += dy * sx;
        h[1][1] += dy * sy;
    }
    h[0][0] /= nf;
    h[0][1] /= nf;
    h[1][0] /= nf;
    h[1][1] /= nf;

    // Closed-form 2D similarity: a = (H00+H11)/var, b = (H10-H01)/var
    let eps = 1e-10;
    let a = (h[0][0] + h[1][1]) / src_var.max(eps);
    let b = (h[1][0] - h[0][1]) / src_var.max(eps);

    // Translation: t = dst_mean - R * src_mean
    let tx = dst_mean[0] - (a * src_mean[0] - b * src_mean[1]);
    let ty = dst_mean[1] - (b * src_mean[0] + a * src_mean[1]);

    [a, -b, tx, b, a, ty]
}

/// Apply a 2x3 affine transform to a source HWC u8 frame using inverse-mapping
/// bilinear interpolation, producing an `(out_h, out_w, 3)` output frame.
fn warp_affine(
    frame: &crate::Frame,
    mat: &[f32; 6],
    out_w: usize,
    out_h: usize,
) -> Result<crate::Frame> {
    let (src_h, src_w, channels) = (frame.shape()[0], frame.shape()[1], frame.shape()[2]);
    anyhow::ensure!(channels == 3, "Expected 3-channel frame, got {channels}");

    // Invert the 2x3 forward matrix so we can do dst→src mapping.
    // mat = [a, -b, tx; b, a, ty]
    let (a, mb, tx, b, aa, ty) = (mat[0], mat[1], mat[2], mat[3], mat[4], mat[5]);
    let det = a * aa - mb * b;
    anyhow::ensure!(det.abs() > 1e-10, "Singular affine matrix (det={det})");
    let inv_det = 1.0 / det;
    let ia =  aa * inv_det;
    let ib = -mb * inv_det;
    let ic = -b  * inv_det;
    let id =  a  * inv_det;
    let itx = -(ia * tx + ib * ty);
    let ity = -(ic * tx + id * ty);

    let mut out = ndarray::Array3::<u8>::zeros((out_h, out_w, 3));

    for dy in 0..out_h {
        for dx in 0..out_w {
            let fx = dx as f32;
            let fy = dy as f32;
            let sx = ia * fx + ib * fy + itx;
            let sy = ic * fx + id * fy + ity;

            let x0 = sx.floor() as i64;
            let y0 = sy.floor() as i64;
            let x1 = x0 + 1;
            let y1 = y0 + 1;

            let wx = sx - x0 as f32; // weight for x1
            let wy = sy - y0 as f32; // weight for y1

            let cx0 = x0.clamp(0, src_w as i64 - 1) as usize;
            let cy0 = y0.clamp(0, src_h as i64 - 1) as usize;
            let cx1 = x1.clamp(0, src_w as i64 - 1) as usize;
            let cy1 = y1.clamp(0, src_h as i64 - 1) as usize;

            for c in 0..3usize {
                let p00 = frame[[cy0, cx0, c]] as f32;
                let p01 = frame[[cy0, cx1, c]] as f32;
                let p10 = frame[[cy1, cx0, c]] as f32;
                let p11 = frame[[cy1, cx1, c]] as f32;

                let val = p00 * (1.0 - wx) * (1.0 - wy)
                    + p01 * wx * (1.0 - wy)
                    + p10 * (1.0 - wx) * wy
                    + p11 * wx * wy;

                out[[dy, dx, c]] = val.clamp(0.0, 255.0) as u8;
            }
        }
    }

    Ok(out)
}

/// Align face using 5 landmarks to canonical 112x112 for ArcFace.
pub fn align_face_arcface(
    frame: &crate::Frame,
    landmarks: &[[f32; 2]; 5],
) -> Result<crate::Frame> {
    let mat = similarity_transform(landmarks, &ARCFACE_DST);
    warp_affine(frame, &mat, 112, 112)
        .context("align_face_arcface: warp_affine failed")
}

/// Align face using 5 landmarks for inswapper (128x128).
pub fn align_face_swap(
    frame: &crate::Frame,
    landmarks: &[[f32; 2]; 5],
) -> Result<crate::Frame> {
    let mat = similarity_transform(landmarks, &SWAP_DST);
    warp_affine(frame, &mat, 128, 128)
        .context("align_face_swap: warp_affine failed")
}

/// Compute forward affine matrix from detected landmarks to the 128x128 swap
/// template.  Exposed for use in `swap.rs` to inverse-warp results back.
pub(crate) fn affine_matrix_swap(landmarks: &[[f32; 2]; 5]) -> [f32; 6] {
    similarity_transform(landmarks, &SWAP_DST)
}

/// Invert a 2x3 affine matrix in-place.
pub(crate) fn invert_affine(mat: &[f32; 6]) -> [f32; 6] {
    let (a, mb, tx, b, aa, ty) = (mat[0], mat[1], mat[2], mat[3], mat[4], mat[5]);
    let det = a * aa - mb * b;
    let inv_det = 1.0 / det;
    let ia =  aa * inv_det;
    let ib = -mb * inv_det;
    let ic = -b  * inv_det;
    let id =  a  * inv_det;
    let itx = -(ia * tx + ib * ty);
    let ity = -(ic * tx + id * ty);
    [ia, ib, itx, ic, id, ity]
}

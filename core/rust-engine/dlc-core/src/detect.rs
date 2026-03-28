//! SCRFD face detection via ONNX Runtime (ort v2 RC).
//!
//! Model: SCRFD-10GF (`buffalo_l/buffalo_l/det_10g.onnx`)
//! - Input:  "input.1"  [1, 3, 640, 640] float32
//! - Output: 9 tensors grouped by type (scores, bboxes, kps), NOT by stride:
//!   index 0: score stride-8   [1,12800,1]
//!   index 1: score stride-16  [1,3200,1]
//!   index 2: score stride-32  [1,800,1]
//!   index 3: bbox  stride-8   [1,12800,4]
//!   index 4: bbox  stride-16  [1,3200,4]
//!   index 5: bbox  stride-32  [1,800,4]
//!   index 6: kps   stride-8   [1,12800,10]
//!   index 7: kps   stride-16  [1,3200,10]
//!   index 8: kps   stride-32  [1,800,10]

use crate::{DetectedFace, Frame};
use crate::preprocess::{preprocess_detection, letterbox_params};
use anyhow::{Context, Result};
use ort::session::Session;
use ort::value::Tensor;

/// IoU threshold for NMS.
const NMS_IOU_THRESHOLD: f32 = 0.4;

/// Anchors per spatial cell (SCRFD-10GF uses 2).
const NUM_ANCHORS: usize = 2;

/// Number of FPN feature-map levels (used as offset stride in output indexing).
const FMC: usize = 3;

/// FPN stride levels: (stride, num_anchors_total).
/// num_anchors_total = (640/stride)^2 * NUM_ANCHORS
const STRIDES: [(usize, usize); 3] = [
    (8,  12800), // 80×80 × 2
    (16,  3200), // 40×40 × 2
    (32,   800), // 20×20 × 2
];

// ── Detector ─────────────────────────────────────────────────────────────────

/// SCRFD-based face detector.
pub struct FaceDetector {
    session: Session,
}

impl FaceDetector {
    /// Load the SCRFD model from `model_path` (det_10g.onnx).
    pub fn new(model_path: &std::path::Path) -> Result<Self> {
        tracing::info!("Loading SCRFD model from {}", model_path.display());

        let session = Session::builder()
            .context("ort Session::builder failed")?
            .commit_from_file(model_path)
            .with_context(|| format!("Failed to load {}", model_path.display()))?;

        for inp in session.inputs() {
            tracing::debug!("model input:  {}  {:?}", inp.name(), inp.dtype());
        }
        for out in session.outputs() {
            tracing::debug!("model output: {}  {:?}", out.name(), out.dtype());
        }
        tracing::info!("SCRFD ready ({} outputs)", session.outputs().len());

        Ok(Self { session })
    }

    /// Detect faces in a BGR `frame`.
    ///
    /// `threshold` — minimum confidence score (recommended: 0.5).
    /// Returns `Vec<DetectedFace>` sorted by descending score after NMS.
    pub fn detect(&mut self, frame: &Frame, threshold: f32) -> Result<Vec<DetectedFace>> {
        let (src_h, src_w, _) = frame.dim();
        tracing::debug!(src_h, src_w, threshold, "FaceDetector::detect");

        // ── 1. Preprocess ────────────────────────────────────────────────────
        let (shape, data) = preprocess_detection(frame)?;
        // Use tuple form: (Vec<i64>, Vec<f32>) — no ndarray feature required.
        let input_tensor = Tensor::<f32>::from_array((shape, data))
            .context("Failed to create SCRFD input tensor")?;

        // ── 2. Inference ─────────────────────────────────────────────────────
        // inputs! returns [SessionInputValue; N] — not Result, no ? here.
        let outputs = self
            .session
            .run(ort::inputs![input_tensor])
            .context("SCRFD inference failed")?;

        tracing::debug!("{} output tensors received", outputs.len());

        // ── 3. Post-process ──────────────────────────────────────────────────
        let (scale, pad_top, pad_left) = letterbox_params(src_h, src_w);

        let mut candidates: Vec<DetectedFace> = Vec::new();

        // Output tensor layout (InsightFace buffalo_l SCRFD-10GF):
        //   Grouped by type, NOT interleaved by stride.
        //   scores: outputs[idx]           (idx = 0..FMC)
        //   bboxes: outputs[idx + FMC]     (idx = 0..FMC)
        //   kps:    outputs[idx + FMC * 2] (idx = 0..FMC)
        for (level_idx, &(stride, num_props)) in STRIDES.iter().enumerate() {
            // try_extract_tensor returns (&Shape, &[T])
            let (_, scores_data) = outputs[level_idx]
                .try_extract_tensor::<f32>()
                .with_context(|| format!("extract scores[{}]", level_idx))?;
            let (_, bboxes_data) = outputs[level_idx + FMC]
                .try_extract_tensor::<f32>()
                .with_context(|| format!("extract bboxes[{}]", level_idx + FMC))?;
            let (_, kps_data) = outputs[level_idx + FMC * 2]
                .try_extract_tensor::<f32>()
                .with_context(|| format!("extract kps[{}]", level_idx + FMC * 2))?;

            // Feature map side length for this stride.
            let feat_side = 640 / stride;

            for anchor_idx in 0..num_props {
                let score = scores_data[anchor_idx];
                if score < threshold {
                    continue;
                }

                // Anchor centre from spatial grid position.
                // anchor_idx = cell * NUM_ANCHORS + anchor_within_cell
                let cell = anchor_idx / NUM_ANCHORS;
                let row  = cell / feat_side;
                let col  = cell % feat_side;
                let cx = col as f32 * stride as f32;
                let cy = row as f32 * stride as f32;

                // SCRFD bbox encoding: (dist_left, dist_top, dist_right, dist_bottom) × stride.
                let b = anchor_idx * 4;
                let x1_lbx = cx - bboxes_data[b]     * stride as f32;
                let y1_lbx = cy - bboxes_data[b + 1] * stride as f32;
                let x2_lbx = cx + bboxes_data[b + 2] * stride as f32;
                let y2_lbx = cy + bboxes_data[b + 3] * stride as f32;

                // Map letterbox coords → original frame coords.
                let x1 = ((x1_lbx - pad_left as f32) / scale).clamp(0.0, src_w as f32);
                let y1 = ((y1_lbx - pad_top  as f32) / scale).clamp(0.0, src_h as f32);
                let x2 = ((x2_lbx - pad_left as f32) / scale).clamp(0.0, src_w as f32);
                let y2 = ((y2_lbx - pad_top  as f32) / scale).clamp(0.0, src_h as f32);

                // Landmarks: 5 × (x, y) = 10 values per anchor.
                let k = anchor_idx * 10;
                let mut landmarks = [[0.0f32; 2]; 5];
                for lm in 0..5 {
                    let lx_lbx = cx + kps_data[k + lm * 2]     * stride as f32;
                    let ly_lbx = cy + kps_data[k + lm * 2 + 1] * stride as f32;
                    landmarks[lm][0] = ((lx_lbx - pad_left as f32) / scale).clamp(0.0, src_w as f32);
                    landmarks[lm][1] = ((ly_lbx - pad_top  as f32) / scale).clamp(0.0, src_h as f32);
                }

                candidates.push(DetectedFace {
                    bbox: [x1, y1, x2, y2],
                    score,
                    landmarks,
                    embedding: None,
                });
            }
        }

        tracing::debug!("{} candidates above threshold {}", candidates.len(), threshold);

        // ── 4. NMS ───────────────────────────────────────────────────────────
        let faces = nms(candidates, NMS_IOU_THRESHOLD);
        tracing::info!("{} face(s) detected", faces.len());
        Ok(faces)
    }
}

// ── NMS ──────────────────────────────────────────────────────────────────────

/// Greedy Non-Maximum Suppression, sorted by descending score.
fn nms(mut candidates: Vec<DetectedFace>, iou_threshold: f32) -> Vec<DetectedFace> {
    candidates.sort_unstable_by(|a, b| {
        b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut kept: Vec<DetectedFace> = Vec::new();

    'outer: for candidate in candidates {
        for kept_face in &kept {
            if iou(&candidate.bbox, &kept_face.bbox) > iou_threshold {
                continue 'outer;
            }
        }
        kept.push(candidate);
    }

    kept
}

/// Intersection-over-Union for axis-aligned bboxes `[x1, y1, x2, y2]`.
fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let ix1 = a[0].max(b[0]);
    let iy1 = a[1].max(b[1]);
    let ix2 = a[2].min(b[2]);
    let iy2 = a[3].min(b[3]);

    let inter = (ix2 - ix1).max(0.0) * (iy2 - iy1).max(0.0);
    if inter == 0.0 {
        return 0.0;
    }

    let area_a = (a[2] - a[0]).max(0.0) * (a[3] - a[1]).max(0.0);
    let area_b = (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0);
    let union  = area_a + area_b - inter;

    if union <= 0.0 { 0.0 } else { inter / union }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn face(x1: f32, y1: f32, x2: f32, y2: f32, score: f32) -> DetectedFace {
        DetectedFace {
            bbox: [x1, y1, x2, y2],
            score,
            landmarks: [[0.0; 2]; 5],
            embedding: None,
        }
    }

    #[test]
    fn iou_identical_boxes() {
        let b = [0.0f32, 0.0, 10.0, 10.0];
        assert!((iou(&b, &b) - 1.0).abs() < 1e-6, "identical boxes must have IoU=1");
    }

    #[test]
    fn iou_no_overlap() {
        let a = [0.0f32, 0.0, 5.0, 5.0];
        let b = [10.0f32, 10.0, 20.0, 20.0];
        assert_eq!(iou(&a, &b), 0.0);
    }

    #[test]
    fn iou_partial_overlap() {
        // 2×2 intersection, areas 25 + 25 - 4 = 46
        let a = [0.0f32, 0.0, 5.0, 5.0];
        let b = [3.0f32, 3.0, 8.0, 8.0];
        let expected = 4.0 / 46.0;
        assert!((iou(&a, &b) - expected).abs() < 1e-6);
    }

    #[test]
    fn nms_suppresses_overlapping() {
        let candidates = vec![
            face(0.0, 0.0, 100.0, 100.0, 0.9),
            face(5.0, 5.0, 105.0, 105.0, 0.8),   // heavy overlap with first → suppressed
            face(200.0, 200.0, 300.0, 300.0, 0.7), // distinct → kept
        ];
        let kept = nms(candidates, NMS_IOU_THRESHOLD);
        assert_eq!(kept.len(), 2);
        assert!((kept[0].score - 0.9).abs() < 1e-6);
        assert!((kept[1].score - 0.7).abs() < 1e-6);
    }

    #[test]
    fn nms_keeps_all_non_overlapping() {
        let candidates: Vec<_> = (0..5)
            .map(|i| face(i as f32 * 200.0, 0.0, i as f32 * 200.0 + 100.0, 100.0, 0.9 - i as f32 * 0.1))
            .collect();
        let kept = nms(candidates, NMS_IOU_THRESHOLD);
        assert_eq!(kept.len(), 5);
    }

    #[test]
    fn nms_sorted_by_score_descending() {
        let candidates = vec![
            face(0.0, 0.0, 10.0, 10.0, 0.6),
            face(500.0, 0.0, 510.0, 10.0, 0.9),
            face(1000.0, 0.0, 1010.0, 10.0, 0.75),
        ];
        let kept = nms(candidates, NMS_IOU_THRESHOLD);
        assert_eq!(kept.len(), 3);
        assert!(kept[0].score >= kept[1].score);
        assert!(kept[1].score >= kept[2].score);
    }
}

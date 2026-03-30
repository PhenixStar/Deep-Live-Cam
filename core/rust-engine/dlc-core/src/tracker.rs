//! Lightweight face tracker — reuses cached detection results to skip
//! expensive SCRFD inference on most frames.
//!
//! Strategy: run full detection every `interval` frames (default 10).
//! On intermediate frames, reuse the last detected faces as-is.
//! At 30fps camera input, faces move <5px between frames — reuse is safe.

use crate::DetectedFace;

/// Tracks faces across frames by caching the last detection result.
pub struct FaceTracker {
    /// How often to run full detection (every N frames). 1 = every frame.
    interval: u32,
    /// Current frame counter (resets to 0 after detection).
    frame_count: u32,
    /// Cached faces from the last detection run.
    cached_faces: Vec<DetectedFace>,
}

impl FaceTracker {
    /// Create a new tracker. `interval` = detect every Nth frame (min 1).
    pub fn new(interval: u32) -> Self {
        Self {
            interval: interval.max(1),
            frame_count: 0,
            cached_faces: Vec::new(),
        }
    }

    /// Update the detection interval at runtime (e.g., from UI slider).
    pub fn set_interval(&mut self, interval: u32) {
        self.interval = interval.max(1);
    }

    /// Returns the current interval.
    pub fn interval(&self) -> u32 {
        self.interval
    }

    /// Returns true if this frame should run full face detection.
    /// Also returns true if the cache is empty (first frame or lost tracking).
    pub fn should_detect(&self) -> bool {
        self.frame_count == 0 || self.cached_faces.is_empty()
    }

    /// Call after a successful detection. Caches the faces and resets counter.
    pub fn update_detected(&mut self, faces: Vec<DetectedFace>) {
        self.cached_faces = faces;
        self.frame_count = 1; // next frame will be 1 (not 0), so it skips
    }

    /// Call on frames where detection is skipped. Returns cached faces.
    /// Advances the frame counter and resets to 0 when interval is reached.
    pub fn get_cached(&mut self) -> &[DetectedFace] {
        self.frame_count += 1;
        if self.frame_count >= self.interval {
            self.frame_count = 0; // trigger detection on next frame
        }
        &self.cached_faces
    }

    /// Force detection on the next frame (e.g., after source face change).
    pub fn invalidate(&mut self) {
        self.frame_count = 0;
        self.cached_faces.clear();
    }

    /// Number of cached faces from last detection.
    pub fn face_count(&self) -> usize {
        self.cached_faces.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_face() -> DetectedFace {
        DetectedFace {
            bbox: [10.0, 20.0, 110.0, 120.0],
            score: 0.95,
            landmarks: [[0.0; 2]; 5],
            embedding: None,
        }
    }

    #[test]
    fn detect_on_first_frame() {
        let t = FaceTracker::new(10);
        assert!(t.should_detect()); // empty cache → must detect
    }

    #[test]
    fn skip_after_detection() {
        let mut t = FaceTracker::new(10);
        t.update_detected(vec![dummy_face()]);
        assert!(!t.should_detect()); // frame_count=1, interval=10
    }

    #[test]
    fn detect_after_interval() {
        let mut t = FaceTracker::new(3);
        t.update_detected(vec![dummy_face()]);
        // frame 1: skip (count=1)
        assert!(!t.should_detect());
        let _ = t.get_cached(); // count becomes 2
        assert!(!t.should_detect());
        let _ = t.get_cached(); // count becomes 3 → resets to 0
        assert!(t.should_detect()); // count=0 → detect
    }

    #[test]
    fn invalidate_forces_detection() {
        let mut t = FaceTracker::new(10);
        t.update_detected(vec![dummy_face()]);
        assert!(!t.should_detect());
        t.invalidate();
        assert!(t.should_detect());
    }

    #[test]
    fn cached_faces_returned() {
        let mut t = FaceTracker::new(10);
        t.update_detected(vec![dummy_face(), dummy_face()]);
        let cached = t.get_cached();
        assert_eq!(cached.len(), 2);
    }
}

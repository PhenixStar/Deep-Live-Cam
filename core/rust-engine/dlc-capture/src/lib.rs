//! Camera capture and virtual camera output.
//!
//! Uses OpenCV (system libopencv via the `opencv` crate) when the `opencv`
//! feature is enabled.  Falls back to a synthetic stub otherwise, so the
//! crate compiles on headless servers where pkg-config cannot locate OpenCV.
//!
//! # Feature flags
//! * `opencv` – links against system libopencv; enables real camera capture.

use anyhow::Result;
use ndarray::Array3;

// ----- public types ---------------------------------------------------------

/// A single captured frame: H × W × C array, BGR channel order, u8 pixels.
pub type Frame = Array3<u8>;

/// Available camera device info.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CameraInfo {
    pub index: u32,
    pub name: String,
}

// ----- list_cameras ---------------------------------------------------------

/// Probe camera indices 0-9 and return those that can be opened.
///
/// Mirrors the Python `cv2.VideoCapture(i).isOpened()` probing loop.
pub fn list_cameras() -> Result<Vec<CameraInfo>> {
    #[cfg(feature = "opencv")]
    {
        list_cameras_opencv()
    }
    #[cfg(not(feature = "opencv"))]
    {
        list_cameras_stub()
    }
}

// ----- CameraCapture --------------------------------------------------------

/// Live camera capture handle.
pub struct CameraCapture {
    inner: CameraCaptureInner,
}

impl CameraCapture {
    /// Open camera at `index` (0-based, matching `/dev/videoN` on Linux).
    pub fn open(index: u32) -> Result<Self> {
        Ok(Self {
            inner: CameraCaptureInner::open(index)?,
        })
    }

    /// Grab one frame.  Returns an `H × W × 3` BGR array.
    pub fn read_frame(&mut self) -> Result<Frame> {
        self.inner.read_frame()
    }

    /// Set capture resolution; best-effort (camera may ignore unsupported sizes).
    pub fn set_resolution(&mut self, width: u32, height: u32) {
        self.inner.set_resolution(width, height);
    }
}

// Drop delegates to inner so the OpenCV path can call cap.release().
impl Drop for CameraCapture {
    fn drop(&mut self) {
        self.inner.release();
    }
}

// ============================================================================
// OpenCV backend
// ============================================================================

#[cfg(feature = "opencv")]
mod opencv_backend {
    use super::*;
    use opencv::{
        core::{Mat, MatTraitConst, CV_8UC3},
        prelude::{VideoCaptureTrait, VideoCaptureTraitConst},
        videoio::{
            VideoCapture, CAP_ANY, CAP_PROP_FRAME_HEIGHT, CAP_PROP_FRAME_WIDTH,
        },
    };

    pub struct CameraCaptureInner {
        cap: VideoCapture,
    }

    impl CameraCaptureInner {
        pub fn open(index: u32) -> Result<Self> {
            let mut cap = VideoCapture::new(index as i32, CAP_ANY)?;
            anyhow::ensure!(
                cap.is_opened()?,
                "camera index {} could not be opened",
                index
            );
            Ok(Self { cap })
        }

        pub fn read_frame(&mut self) -> Result<Frame> {
            let mut mat = Mat::default();
            anyhow::ensure!(
                VideoCaptureTrait::read(&mut self.cap, &mut mat)?,
                "VideoCapture::read returned false (end of stream or camera error)"
            );
            anyhow::ensure!(
                mat.typ() == CV_8UC3,
                "unexpected Mat type {}; expected CV_8UC3",
                mat.typ()
            );
            let rows = mat.rows() as usize;
            let cols = mat.cols() as usize;
            // SAFETY: Mat data is contiguous (CV_8UC3, rows × cols × 3 bytes).
            let slice = unsafe {
                std::slice::from_raw_parts(mat.data(), rows * cols * 3)
            };
            let arr = Array3::from_shape_vec((rows, cols, 3), slice.to_vec())?;
            Ok(arr)
        }

        pub fn set_resolution(&mut self, width: u32, height: u32) {
            // Errors are silently ignored; the next read will reveal the actual size.
            let _ = VideoCaptureTrait::set(
                &mut self.cap,
                CAP_PROP_FRAME_WIDTH,
                width as f64,
            );
            let _ = VideoCaptureTrait::set(
                &mut self.cap,
                CAP_PROP_FRAME_HEIGHT,
                height as f64,
            );
        }

        pub fn release(&mut self) {
            let _ = VideoCaptureTrait::release(&mut self.cap);
        }
    }

    pub fn list_cameras_opencv() -> Result<Vec<CameraInfo>> {
        let mut cameras = Vec::new();
        // Probe only 0-3 to avoid long hangs on Windows (each failed index
        // can block for several seconds with DirectShow/MSMF backends).
        for i in 0..4u32 {
            if let Ok(cap) = VideoCapture::new(i as i32, CAP_ANY) {
                if cap.is_opened().unwrap_or(false) {
                    cameras.push(CameraInfo {
                        index: i,
                        name: format!("Camera {}", i),
                    });
                }
            }
        }
        Ok(cameras)
    }
}

// ============================================================================
// Stub backend  (no opencv feature)
// ============================================================================

#[cfg(not(feature = "opencv"))]
mod stub_backend {
    use super::*;

    /// Generates synthetic 640 × 480 BGR test frames (grey gradient + frame counter).
    pub struct CameraCaptureInner {
        index: u32,
        frame_count: u64,
        width: u32,
        height: u32,
    }

    impl CameraCaptureInner {
        pub fn open(index: u32) -> Result<Self> {
            tracing::warn!(
                "dlc-capture compiled without `opencv` feature; \
                 returning synthetic frames for camera index {}",
                index
            );
            Ok(Self {
                index,
                frame_count: 0,
                width: 640,
                height: 480,
            })
        }

        pub fn read_frame(&mut self) -> Result<Frame> {
            let h = self.height as usize;
            let w = self.width as usize;
            // Simple grey gradient; column index drives intensity so the image
            // is visually distinct at different resolutions.
            let data: Vec<u8> = (0..h * w * 3)
                .map(|i| {
                    let pixel = i / 3;
                    let col = pixel % w;
                    // Vary lightly with frame_count so callers can detect motion.
                    ((col + self.frame_count as usize) % 256) as u8
                })
                .collect();
            self.frame_count = self.frame_count.wrapping_add(1);
            Ok(Array3::from_shape_vec((h, w, 3), data)?)
        }

        pub fn set_resolution(&mut self, width: u32, height: u32) {
            self.width = width;
            self.height = height;
        }

        pub fn release(&mut self) {
            tracing::debug!("stub camera {} released", self.index);
        }
    }

    pub fn list_cameras_stub() -> Result<Vec<CameraInfo>> {
        // Expose one synthetic camera so callers always have something to open.
        Ok(vec![CameraInfo {
            index: 0,
            name: "Stub Camera (opencv feature disabled)".into(),
        }])
    }
}

// ============================================================================
// Wire up the selected backend
// ============================================================================

#[cfg(feature = "opencv")]
use opencv_backend::CameraCaptureInner;
#[cfg(feature = "opencv")]
use opencv_backend::list_cameras_opencv;

#[cfg(not(feature = "opencv"))]
use stub_backend::CameraCaptureInner;
#[cfg(not(feature = "opencv"))]
use stub_backend::list_cameras_stub;

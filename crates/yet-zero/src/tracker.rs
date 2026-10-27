// SPDX-License-Identifier: GPL-3.0-only

//! Camera polling and eye-ROI previews.
//!
//! The camera owns its capture thread. [`Tracker::poll`] only consumes a frame
//! when the capture counter advanced, so a 60 Hz controller never counts the
//! same frame twice. Preview images are downscaled on the controller thread;
//! the UI only uploads a texture.

use std::time::{Duration, Instant};

use crate::camera::{Camera, CameraInfo};
use yet_core::{quad_brightness, EyeRegion, GrayFrame, QuadBrightness};
use yet_stim::CameraConfig;

use crate::live::{capture_config, eye_region, LiveError};

/// Maximum preview width in pixels.
pub const PREVIEW_MAX_WIDTH: u32 = 480;

/// Maximum preview height in pixels.
pub const PREVIEW_MAX_HEIGHT: u32 = 320;

/// One fresh camera frame and its quadrant features.
#[derive(Clone, Debug)]
pub struct PolledFrame {
    /// The grayscale frame.
    pub frame: GrayFrame,
    /// Quadrant means for the configured eye region.
    pub features: QuadBrightness,
}

/// A camera plus the configured eye region.
pub struct Tracker {
    camera: Camera,
    roi: EyeRegion,
    last_captured: u64,
    opened_at: Instant,
    last_frame_at: Option<Instant>,
}

impl Tracker {
    /// Open the configured camera and resolve the eye region.
    pub fn open(config: &CameraConfig, device: Option<usize>) -> Result<Self, LiveError> {
        let camera = Camera::open(capture_config(config, device))?;
        let roi = eye_region(config, camera.info())?;
        let last_captured = camera.stats().captured;
        Ok(Self {
            camera,
            roi,
            last_captured,
            opened_at: Instant::now(),
            last_frame_at: None,
        })
    }

    /// Negotiated camera metadata.
    pub fn info(&self) -> &CameraInfo {
        self.camera.info()
    }

    /// Eye region used for features and the preview.
    pub const fn roi(&self) -> EyeRegion {
        self.roi
    }

    /// Capture rate since opening the camera.
    pub fn capture_fps(&self) -> Option<f32> {
        let seconds = self.opened_at.elapsed().as_secs_f32();
        if seconds < 0.5 {
            return None;
        }
        Some(self.camera.stats().captured as f32 / seconds)
    }

    /// Age of the most recent consumed frame.
    pub fn last_frame_age(&self, now: Instant) -> Option<Duration> {
        self.last_frame_at
            .map(|then| now.saturating_duration_since(then))
    }

    /// Consume the latest frame if the capture counter advanced.
    pub fn poll(&mut self) -> Option<PolledFrame> {
        let captured = self.camera.stats().captured;
        if captured == self.last_captured {
            return None;
        }
        self.last_captured = captured;
        let frame = self.camera.latest_frame()?;
        let features = quad_brightness(&frame, self.roi)?;
        self.last_frame_at = Some(Instant::now());
        Some(PolledFrame { frame, features })
    }

    /// Crop the eye region and downscale it for the console preview.
    pub fn preview(&self, frame: &GrayFrame) -> Option<(u32, u32, Vec<u8>)> {
        let crop = frame.crop(self.roi)?;
        thumbnail(&crop, PREVIEW_MAX_WIDTH, PREVIEW_MAX_HEIGHT)
    }
}

/// Nearest-neighbor downscale of a grayscale frame.
///
/// Returns `None` for an empty frame. Images smaller than the bounds are
/// returned at their natural size.
pub fn thumbnail(
    frame: &GrayFrame,
    max_width: u32,
    max_height: u32,
) -> Option<(u32, u32, Vec<u8>)> {
    let width = frame.width();
    let height = frame.height();
    if width == 0 || height == 0 || max_width == 0 || max_height == 0 {
        return None;
    }

    let scale = (max_width as f32 / width as f32)
        .min(max_height as f32 / height as f32)
        .min(1.0);
    let target_width = ((width as f32 * scale).round() as u32).max(1);
    let target_height = ((height as f32 * scale).round() as u32).max(1);
    let mut data = Vec::with_capacity((target_width * target_height) as usize);

    for row in 0..target_height {
        let source_y = ((row as f32 + 0.5) / scale) as u32;
        let source_y = source_y.min(height - 1);
        for column in 0..target_width {
            let source_x = ((column as f32 + 0.5) / scale) as u32;
            let source_x = source_x.min(width - 1);
            data.push(frame.pixel(source_x, source_y).unwrap_or(0));
        }
    }

    Some((target_width, target_height, data))
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::{Duration, Instant};

    use yet_core::GrayFrame;
    use yet_stim::CameraConfig;

    use super::{thumbnail, Tracker};

    #[test]
    fn a_large_frame_is_downscaled_to_the_bounds() {
        let frame = GrayFrame::new(8, 4, (0..32).collect()).unwrap();

        let (width, height, data) = thumbnail(&frame, 4, 2).unwrap();

        assert_eq!((width, height), (4, 2));
        assert_eq!(data.len(), 8);
        assert_eq!(data[0], frame.pixel(1, 1).unwrap());
        assert_eq!(data[7], frame.pixel(7, 3).unwrap());
    }

    #[test]
    fn a_small_frame_keeps_its_size() {
        let frame = GrayFrame::new(2, 2, vec![1, 2, 3, 4]).unwrap();

        let (width, height, data) = thumbnail(&frame, 240, 160).unwrap();

        assert_eq!((width, height), (2, 2));
        assert_eq!(data, vec![1, 2, 3, 4]);
    }

    #[test]
    fn empty_bounds_have_no_thumbnail() {
        let frame = GrayFrame::new(2, 2, vec![0; 4]).unwrap();

        assert!(thumbnail(&frame, 0, 10).is_none());
        assert!(thumbnail(&frame, 10, 0).is_none());
    }

    /// Hardware smoke test for the GEMBIRD/AppoTech AX2311 (`/dev/video2`).
    ///
    /// Run with the YET camera attached:
    /// `cargo test -p yet-zero --all-features -- --ignored the_yet_camera_delivers_frames`
    /// Set `YET_TEST_DEVICE` to use a different capture index.
    #[test]
    #[ignore = "requires the YET camera; set YET_TEST_DEVICE to override the index"]
    fn the_yet_camera_delivers_frames() {
        let device = std::env::var("YET_TEST_DEVICE")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(2);
        let config = CameraConfig {
            usb_index: device,
            ..CameraConfig::default()
        };
        let mut tracker = Tracker::open(&config, None).expect("the camera opens");
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut polled = 0_u32;

        while Instant::now() < deadline && polled < 10 {
            if let Some(frame) = tracker.poll() {
                for value in frame.features.to_array() {
                    assert!(
                        (0.0..=255.0).contains(&value),
                        "quadrant mean {value} is outside a gray frame"
                    );
                }
                let (width, height, data) = tracker.preview(&frame.frame).expect("a preview");
                assert!(width > 0 && height > 0);
                assert_eq!(data.len(), (width * height) as usize);
                polled += 1;
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(polled >= 10, "only {polled} frames arrived in 3 s");
        assert!(
            tracker.capture_fps().unwrap_or(0.0) > 1.0,
            "capture stopped: {:?} fps",
            tracker.capture_fps()
        );
    }
}

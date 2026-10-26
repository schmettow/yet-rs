// SPDX-License-Identifier: GPL-3.0-only

//! Camera capture with a latest-frame mailbox.
//!
//! The platform camera is opened and owned by a dedicated capture thread. That
//! thread publishes the newest [`GrayFrame`] into a one-slot mailbox; the
//! tracker takes the latest frame without ever blocking on the device. If the
//! consumer is slower than the camera, the older frame is dropped and counted.
//!
//! Platform adapters implement `CameraSource` and never leave this module's
//! thread. The tracker and the engine see only [`Camera`], [`GrayFrame`] and
//! [`CameraInfo`], never `linuxvideo`, `nokhwa` or an `image` type.

mod convert;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod nokhwa;

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Deserialize;
use thiserror::Error;
use yet_present::SessionTime;

use crate::tracking::{EyeRegion, GrayFrame};

/// Default normalized eye region: the centered half of the frame.
///
/// This is the ROI the SPK-3 camera spike used (320×240 inside 640×480).
pub const DEFAULT_EYE_REGION: [f32; 4] = [0.25, 0.25, 0.5, 0.5];

/// Stop the capture loop after this many consecutive device errors.
const MAX_CONSECUTIVE_ERRORS: u32 = 30;

/// Role of a camera in a rig.
///
/// v1 has one [`Self::EyeLeft`] camera. Future binocular tracking adds
/// [`Self::EyeRight`]; a head-mounted world camera uses [`Self::World`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CameraRole {
    /// Camera viewing the left eye.
    EyeLeft,
    /// Camera viewing the right eye.
    EyeRight,
    /// Head-mounted camera viewing the world.
    World,
}

/// One borrowed frame tagged with its camera role and capture time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraFrame<'a> {
    /// Which camera produced the frame.
    pub role: CameraRole,
    /// Session-relative capture time.
    pub captured_at: SessionTime,
    /// Grayscale frame.
    pub frame: &'a GrayFrame,
}

/// A set of camera frames at one reference time.
///
/// Multi-camera fusion uses the timestamps, not an assumption of hardware sync.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameSet<'a> {
    /// Frames in rig order.
    pub frames: &'a [CameraFrame<'a>],
    /// Reference time for features extracted from this set.
    pub reference_at: SessionTime,
}

impl<'a> FrameSet<'a> {
    /// The first frame for a role.
    pub fn frame(&self, role: CameraRole) -> Option<&'a CameraFrame<'a>> {
        self.frames.iter().find(|frame| frame.role == role)
    }

    /// Number of frames.
    pub const fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether the set is empty.
    pub const fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

/// A role-tagged set of cameras.
///
/// v1 wraps one [`CameraRole::EyeLeft`] camera. The rig is the seam for
/// binocular Octabright and a head-mounted world camera; it does not imply that
/// multiple cameras are wired or synchronized yet.
#[derive(Debug, Default)]
pub struct CameraRig {
    cameras: Vec<(CameraRole, Camera)>,
}

impl CameraRig {
    /// Create an empty rig.
    pub const fn new() -> Self {
        Self {
            cameras: Vec::new(),
        }
    }

    /// Wrap one camera.
    pub fn single(role: CameraRole, camera: Camera) -> Self {
        Self {
            cameras: vec![(role, camera)],
        }
    }

    /// Add a camera for a role.
    pub fn add(&mut self, role: CameraRole, camera: Camera) {
        self.cameras.push((role, camera));
    }

    /// The camera for a role.
    pub fn camera(&self, role: CameraRole) -> Option<&Camera> {
        self.cameras
            .iter()
            .find(|(candidate, _)| *candidate == role)
            .map(|(_, camera)| camera)
    }

    /// The camera for a role, mutably.
    pub fn camera_mut(&mut self, role: CameraRole) -> Option<&mut Camera> {
        self.cameras
            .iter_mut()
            .find(|(candidate, _)| *candidate == role)
            .map(|(_, camera)| camera)
    }

    /// Roles in rig order.
    pub fn roles(&self) -> impl Iterator<Item = CameraRole> + '_ {
        self.cameras.iter().map(|(role, _)| *role)
    }

    /// Number of cameras.
    pub fn len(&self) -> usize {
        self.cameras.len()
    }

    /// Whether the rig has no cameras.
    pub fn is_empty(&self) -> bool {
        self.cameras.is_empty()
    }
}

/// A requested capture format, from `config.toml`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreferredFormat {
    /// Prefer grayscale, then YUYV, then MJPEG.
    #[default]
    Auto,
    /// Ask for a grayscale format such as GREY, Y800 or Y8.
    Gray,
    /// Ask for YUYV 4:2:2.
    Yuyv,
    /// Ask for MJPEG.
    Mjpeg,
}

impl fmt::Display for PreferredFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => f.write_str("auto"),
            Self::Gray => f.write_str("gray"),
            Self::Yuyv => f.write_str("yuyv"),
            Self::Mjpeg => f.write_str("mjpeg"),
        }
    }
}

/// The pixel format the driver actually negotiated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// One byte per pixel.
    Gray,
    /// Packed Y0 U Y1 V, two bytes per pixel.
    Yuyv,
    /// JPEG-compressed frames.
    Mjpeg,
}

impl fmt::Display for PixelFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gray => f.write_str("gray"),
            Self::Yuyv => f.write_str("yuyv"),
            Self::Mjpeg => f.write_str("mjpeg"),
        }
    }
}

/// Capture settings for one camera.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureConfig {
    /// Zero-based device index, e.g. `/dev/video0` on Linux.
    pub device: usize,
    /// Requested frame width.
    pub width: u32,
    /// Requested frame height.
    pub height: u32,
    /// Requested frame rate.
    pub fps: u32,
    /// Format preference.
    pub format: PreferredFormat,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            device: 0,
            width: 640,
            height: 480,
            fps: 30,
            format: PreferredFormat::Auto,
        }
    }
}

impl CaptureConfig {
    /// Validate dimensions and frame rate.
    pub fn validate(&self) -> Result<(), CameraError> {
        if self.width == 0 || self.height == 0 {
            return Err(CameraError::InvalidDimensions {
                width: self.width,
                height: self.height,
            });
        }
        if self.fps == 0 {
            return Err(CameraError::Config("camera fps must be positive".into()));
        }
        Ok(())
    }
}

/// Metadata for a negotiated camera stream.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraInfo {
    /// Device path or name.
    pub device: String,
    /// Human-readable camera name.
    pub name: String,
    /// Negotiated width.
    pub width: u32,
    /// Negotiated height.
    pub height: u32,
    /// Raw row stride in bytes; `0` for compressed formats.
    pub stride: u32,
    /// Negotiated frame rate, when reported by the driver.
    pub fps: Option<f32>,
    /// Negotiated pixel format.
    pub format: PixelFormat,
}

impl CameraInfo {
    /// Convert a normalized `[x, y, width, height]` region to pixels.
    ///
    /// Returns `None` when the region is not finite, leaves the `0..=1` square,
    /// is empty, or rounds to fewer than two pixels in either direction (a
    /// quadrant would be empty).
    pub fn eye_region(&self, normalized: [f32; 4]) -> Option<EyeRegion> {
        if !validate_eye_region(normalized) {
            return None;
        }
        let [x, y, width, height] = normalized;
        let left = (x * self.width as f32).round() as u32;
        let top = (y * self.height as f32).round() as u32;
        let right = (((x + width) * self.width as f32).round() as u32).min(self.width);
        let bottom = (((y + height) * self.height as f32).round() as u32).min(self.height);
        let width = right.saturating_sub(left);
        let height = bottom.saturating_sub(top);
        if width < 2 || height < 2 {
            return None;
        }
        Some(EyeRegion::new(left, top, width, height))
    }
}

/// Whether a normalized eye region is finite and inside the unit square.
pub fn validate_eye_region(normalized: [f32; 4]) -> bool {
    let [x, y, width, height] = normalized;
    [x, y, width, height].iter().all(|value| value.is_finite())
        && x >= 0.0
        && y >= 0.0
        && width > 0.0
        && height > 0.0
        && x + width <= 1.0
        && y + height <= 1.0
}

/// A snapshot of capture counters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CameraStats {
    /// Frames successfully converted and published.
    pub captured: u64,
    /// Frames rejected as corrupt, blank or undecodable.
    pub corrupt: u64,
    /// Frames replaced in the mailbox before the consumer took them.
    pub dropped: u64,
    /// Device errors.
    pub errors: u64,
    /// Most recent device error.
    pub last_error: Option<String>,
}

/// Errors from opening or reading a camera.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum CameraError {
    /// The device could not be opened or negotiated.
    #[error("camera `{device}` could not be opened: {message}")]
    Open {
        /// Device path or name.
        device: String,
        /// Platform error message.
        message: String,
    },
    /// No supported pixel format is available.
    #[error("camera `{device}` has no supported format (available: {available})")]
    NoFormat {
        /// Device path or name.
        device: String,
        /// Comma-separated available formats.
        available: String,
    },
    /// Reading a frame from the device failed.
    #[error("camera `{device}` read failed: {message}")]
    Read {
        /// Device path or name.
        device: String,
        /// Platform error message.
        message: String,
    },
    /// The negotiated format is not implemented.
    #[error("camera format `{0}` is not supported")]
    UnsupportedFormat(String),
    /// A frame buffer is too short for the declared layout.
    #[error("frame is truncated: expected at least {expected} bytes, got {actual}")]
    Truncated {
        /// Required bytes.
        expected: usize,
        /// Available bytes.
        actual: usize,
    },
    /// Width or height is zero.
    #[error("invalid camera dimensions {width}x{height}")]
    InvalidDimensions {
        /// Requested width.
        width: u32,
        /// Requested height.
        height: u32,
    },
    /// The capture configuration is invalid.
    #[error("invalid capture configuration: {0}")]
    Config(String),
    /// macOS camera permission was denied.
    #[error("camera permission was denied")]
    PermissionDenied,
    /// macOS camera permission did not arrive in time.
    #[error("camera permission request timed out")]
    PermissionTimeout,
    /// MJPEG decoding failed.
    #[error("MJPEG decode failed: {0}")]
    Decode(String),
    /// This platform has no camera backend.
    #[error("camera capture is not supported on this platform")]
    UnsupportedPlatform,
    /// The capture thread could not be started.
    #[error("could not start the camera thread: {0}")]
    Thread(String),
}

/// A blocking source of grayscale frames, implemented by platform adapters.
pub(crate) trait CameraSource {
    /// Read one frame. `Ok(None)` means the frame was corrupt or blank.
    fn next_frame(&mut self) -> Result<Option<GrayFrame>, CameraError>;

    /// Negotiated stream metadata.
    fn info(&self) -> &CameraInfo;
}

/// A running camera whose newest frame can be taken without blocking.
#[derive(Debug)]
pub struct Camera {
    mailbox: Arc<Mailbox>,
    counters: Arc<Counters>,
    stop: Arc<AtomicBool>,
    info: CameraInfo,
    thread: Option<JoinHandle<()>>,
}

impl Camera {
    /// Open the configured camera and start the capture thread.
    ///
    /// Opening and format negotiation run on the capture thread; this call
    /// blocks until they finish or fail.
    pub fn open(config: CaptureConfig) -> Result<Self, CameraError> {
        config.validate()?;

        let counters = Arc::new(Counters::default());
        let mailbox = Arc::new(Mailbox::new(Arc::clone(&counters)));
        let stop = Arc::new(AtomicBool::new(false));
        let (info_sender, info_receiver) = mpsc::sync_channel(0);

        let thread = thread::Builder::new()
            .name("yet-camera".into())
            .spawn({
                let mailbox = Arc::clone(&mailbox);
                let counters = Arc::clone(&counters);
                let stop = Arc::clone(&stop);
                move || match open_platform(&config, Arc::clone(&counters)) {
                    Ok(mut source) => {
                        let info = source.info().clone();
                        if info_sender.send(Ok(info)).is_err() {
                            return;
                        }
                        run(source.as_mut(), &mailbox, &counters, &stop);
                    }
                    Err(error) => {
                        let _ = info_sender.send(Err(error));
                    }
                }
            })
            .map_err(|error| CameraError::Thread(error.to_string()))?;

        let info = match info_receiver.recv() {
            Ok(Ok(info)) => info,
            Ok(Err(error)) => {
                let _ = thread.join();
                return Err(error);
            }
            Err(_) => {
                let _ = thread.join();
                return Err(CameraError::Thread(
                    "capture thread exited before reporting a format".into(),
                ));
            }
        };

        Ok(Self {
            mailbox,
            counters,
            stop,
            info,
            thread: Some(thread),
        })
    }

    /// Negotiated stream metadata.
    pub fn info(&self) -> &CameraInfo {
        &self.info
    }

    /// A snapshot of capture counters.
    pub fn stats(&self) -> CameraStats {
        self.counters.snapshot()
    }

    /// Take the newest frame, if one has arrived since the last call.
    pub fn latest_frame(&self) -> Option<GrayFrame> {
        self.mailbox.take_latest()
    }

    /// Stop the capture thread and wait for it.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(source: &mut dyn CameraSource, mailbox: &Mailbox, counters: &Counters, stop: &AtomicBool) {
    let mut consecutive_errors = 0_u32;

    while !stop.load(Ordering::Relaxed) {
        match source.next_frame() {
            Ok(Some(frame)) => {
                consecutive_errors = 0;
                mailbox.publish(frame);
            }
            Ok(None) => {
                consecutive_errors = 0;
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => {
                consecutive_errors += 1;
                counters.record_error(&error);
                if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

#[derive(Debug, Default)]
struct Counters {
    captured: AtomicU64,
    corrupt: AtomicU64,
    dropped: AtomicU64,
    errors: AtomicU64,
    last_error: Mutex<Option<String>>,
}

impl Counters {
    fn captured(&self) {
        self.captured.fetch_add(1, Ordering::Relaxed);
    }

    fn corrupt(&self) {
        self.corrupt.fetch_add(1, Ordering::Relaxed);
    }

    fn dropped(&self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }

    fn record_error(&self, error: &CameraError) {
        self.errors.fetch_add(1, Ordering::Relaxed);
        *self.last_error() = Some(error.to_string());
    }

    fn last_error(&self) -> MutexGuard<'_, Option<String>> {
        self.last_error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn snapshot(&self) -> CameraStats {
        CameraStats {
            captured: self.captured.load(Ordering::Relaxed),
            corrupt: self.corrupt.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            last_error: self.last_error().clone(),
        }
    }
}

#[derive(Debug)]
struct Mailbox {
    frame: Mutex<Option<GrayFrame>>,
    counters: Arc<Counters>,
}

impl Mailbox {
    fn new(counters: Arc<Counters>) -> Self {
        Self {
            frame: Mutex::new(None),
            counters,
        }
    }

    fn publish(&self, frame: GrayFrame) {
        let mut slot = self
            .frame
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if slot.is_some() {
            self.counters.dropped();
        }
        *slot = Some(frame);
        self.counters.captured();
    }

    fn take_latest(&self) -> Option<GrayFrame> {
        self.frame
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }
}

/// Whether a frame is entirely (or overwhelmingly) black.
///
/// The Python prototype rejects a frame whose sum is zero. Sampling every 64th
/// byte keeps this cheap while still catching an all-black frame.
fn is_blank(frame: &GrayFrame) -> bool {
    frame.data().iter().step_by(64).all(|&value| value == 0)
}

#[cfg(target_os = "linux")]
fn open_platform(
    config: &CaptureConfig,
    counters: Arc<Counters>,
) -> Result<Box<dyn CameraSource>, CameraError> {
    linux::open(config, counters)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn open_platform(
    config: &CaptureConfig,
    counters: Arc<Counters>,
) -> Result<Box<dyn CameraSource>, CameraError> {
    nokhwa::open(config, counters)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn open_platform(
    _config: &CaptureConfig,
    _counters: Arc<Counters>,
) -> Result<Box<dyn CameraSource>, CameraError> {
    Err(CameraError::UnsupportedPlatform)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    #[test]
    fn an_empty_rig_has_no_cameras() {
        let rig = CameraRig::new();

        assert!(rig.is_empty());
        assert_eq!(rig.len(), 0);
        assert!(rig.camera(CameraRole::EyeLeft).is_none());
        assert_eq!(rig.roles().count(), 0);
    }

    fn info() -> CameraInfo {
        CameraInfo {
            device: "mock".into(),
            name: "mock".into(),
            width: 1,
            height: 1,
            stride: 1,
            fps: Some(30.0),
            format: PixelFormat::Gray,
        }
    }

    fn frame(value: u8) -> GrayFrame {
        GrayFrame::new(1, 1, vec![value]).unwrap()
    }

    #[test]
    fn the_mailbox_keeps_only_the_latest_frame() {
        let counters = Arc::new(Counters::default());
        let mailbox = Mailbox::new(Arc::clone(&counters));

        mailbox.publish(frame(1));
        mailbox.publish(frame(2));

        assert_eq!(mailbox.take_latest().unwrap().pixel(0, 0), Some(2));
        assert_eq!(
            counters.snapshot(),
            CameraStats {
                captured: 2,
                corrupt: 0,
                dropped: 1,
                errors: 0,
                last_error: None,
            }
        );
    }

    #[test]
    fn an_all_black_frame_is_blank() {
        assert!(is_blank(&GrayFrame::new(4, 4, vec![0; 16]).unwrap()));

        // The check samples every 64th byte; put the non-zero value on a
        // sampled offset.
        let mut data = vec![0; 128];
        data[64] = 1;
        assert!(!is_blank(&GrayFrame::new(16, 8, data).unwrap()));
    }

    #[test]
    fn the_capture_loop_publishes_frames_and_stops() {
        struct MockSource {
            frames: VecDeque<Result<Option<GrayFrame>, CameraError>>,
            info: CameraInfo,
        }

        impl CameraSource for MockSource {
            fn next_frame(&mut self) -> Result<Option<GrayFrame>, CameraError> {
                self.frames.pop_front().unwrap_or(Ok(None))
            }

            fn info(&self) -> &CameraInfo {
                &self.info
            }
        }

        let counters = Arc::new(Counters::default());
        let mailbox = Arc::new(Mailbox::new(Arc::clone(&counters)));
        let stop = Arc::new(AtomicBool::new(false));
        let handle = thread::spawn({
            let mailbox = Arc::clone(&mailbox);
            let counters = Arc::clone(&counters);
            let stop = Arc::clone(&stop);
            let info = info();
            move || {
                let mut frames = VecDeque::new();
                frames.push_back(Ok(Some(frame(7))));
                frames.push_back(Ok(Some(frame(9))));
                let mut source = MockSource { frames, info };
                run(&mut source, &mailbox, &counters, &stop);
            }
        });

        let mut captured = 0;
        for _ in 0..200 {
            captured = counters.snapshot().captured;
            if captured == 2 {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(captured, 2);

        stop.store(true, Ordering::Relaxed);
        handle.join().unwrap();

        assert_eq!(mailbox.take_latest().unwrap().pixel(0, 0), Some(9));
        assert_eq!(counters.snapshot().dropped, 1);
    }
}

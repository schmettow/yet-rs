// SPDX-License-Identifier: GPL-3.0-only

//! macOS and Windows camera adapter using `nokhwa`.
//!
//! Only the native backends are enabled: `input-avfoundation` on macOS and
//! `input-msmf` on Windows. The Linux backend is deliberately absent because it
//! would pull in `v4l`/bindgen; Linux uses `linuxvideo` instead.
//!
//! The camera is created, read and dropped inside the capture thread. `nokhwa`
//! does not require the camera to be moved between threads.

use std::sync::Arc;

use nokhwa::utils::{CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType};
use nokhwa::Camera as NokhwaCamera;

use yet_core::GrayFrame;

use super::convert::{gray_to_gray, mjpeg_to_gray, yuyv_to_gray};
use super::{
    CameraError, CameraInfo, CameraSource, CaptureConfig, Counters, PixelFormat, PreferredFormat,
};

/// Every format the Windows backend can report.
///
/// Unsupported formats are included so that opening the device for enumeration
/// succeeds even when it offers no format this adapter can convert; the caller
/// then gets a [`CameraError::NoFormat`] list instead of a negotiation error.
#[cfg(target_os = "windows")]
const PROBE_FORMATS: [FrameFormat; 6] = [
    FrameFormat::GRAY,
    FrameFormat::YUYV,
    FrameFormat::MJPEG,
    FrameFormat::NV12,
    FrameFormat::RAWRGB,
    FrameFormat::RAWBGR,
];

pub(super) fn open(
    config: &CaptureConfig,
    counters: Arc<Counters>,
) -> Result<Box<dyn CameraSource>, CameraError> {
    #[cfg(target_os = "macos")]
    ensure_permission()?;

    let device = format!("camera {}", config.device);
    let index = CameraIndex::Index(config.device as u32);

    #[cfg(target_os = "windows")]
    {
        open_windows(index, device, config, counters)
    }

    #[cfg(target_os = "macos")]
    {
        open_macos(index, device, config, counters)
    }
}

/// Open a Media Foundation camera by choosing from its own format list.
///
/// `nokhwa`'s `RequestedFormatType::Closest` matches only the nominal frame
/// format and computes frame rates against the requested, not the closest,
/// resolution. A device that exposes the requested resolution in a different
/// format therefore fails with "failed to fulfil requested format". Media
/// Foundation cameras expose MJPEG on some machines and only YUYV/NV12 on
/// others, so the adapter enumerates the device and falls back in the same
/// order as the Linux adapter: gray, YUYV, MJPEG.
#[cfg(target_os = "windows")]
#[allow(deprecated)] // `set_camera_requset` uses the trait list that MSMF caps at two formats.
fn open_windows(
    index: CameraIndex,
    device: String,
    config: &CaptureConfig,
    counters: Arc<Counters>,
) -> Result<Box<dyn CameraSource>, CameraError> {
    let probe = RequestedFormat::with_formats(RequestedFormatType::None, &PROBE_FORMATS);
    let mut camera = NokhwaCamera::new(index, probe).map_err(|error| CameraError::Open {
        device: device.clone(),
        message: error.to_string(),
    })?;

    let available = available_formats(&mut camera);
    let candidates = candidates(config, &available);
    if candidates.is_empty() {
        return Err(CameraError::NoFormat {
            device: device.clone(),
            available: describe_formats(&available),
        });
    }

    let mut last_error = None;
    let mut chosen = None;
    for candidate in candidates {
        match camera.set_camera_format(candidate) {
            Ok(()) => {
                chosen = Some(candidate);
                break;
            }
            Err(error) => last_error = Some(error),
        }
    }
    let Some(chosen) = chosen else {
        return Err(CameraError::Open {
            device,
            message: last_error.map_or_else(
                || "the device reported no settable format".to_owned(),
                |error| error.to_string(),
            ),
        });
    };

    camera.open_stream().map_err(|error| CameraError::Open {
        device: device.clone(),
        message: error.to_string(),
    })?;

    Ok(Box::new(NokhwaSource {
        camera,
        info: info_from_format(&device, chosen)?,
        counters,
    }))
}

/// Open an AVFoundation camera with the closest requested format.
#[cfg(target_os = "macos")]
fn open_macos(
    index: CameraIndex,
    device: String,
    config: &CaptureConfig,
    counters: Arc<Counters>,
) -> Result<Box<dyn CameraSource>, CameraError> {
    let nominal = CameraFormat::new(
        nokhwa::utils::Resolution::new(config.width, config.height),
        nominal_format(config.format),
        config.fps,
    );
    let requested = RequestedFormat::with_formats(
        RequestedFormatType::Closest(nominal),
        wanted_formats(config.format),
    );

    let mut camera = NokhwaCamera::new(index, requested).map_err(|error| CameraError::Open {
        device: device.clone(),
        message: error.to_string(),
    })?;
    camera.open_stream().map_err(|error| CameraError::Open {
        device: device.clone(),
        message: error.to_string(),
    })?;

    let info = info_from_format(&device, camera.camera_format())?;
    Ok(Box::new(NokhwaSource {
        camera,
        info,
        counters,
    }))
}

/// Enumerate the formats Media Foundation reports for an open device.
#[cfg(target_os = "windows")]
fn available_formats(camera: &mut NokhwaCamera) -> Vec<CameraFormat> {
    let mut found = Vec::new();
    for format in PROBE_FORMATS {
        let Ok(by_resolution) = camera.compatible_list_by_resolution(format) else {
            continue;
        };
        for (resolution, frame_rates) in by_resolution {
            for frame_rate in frame_rates {
                found.push(CameraFormat::new(resolution, format, frame_rate));
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Order the device formats by preference, resolution distance and frame rate.
#[cfg(target_os = "windows")]
fn candidates(config: &CaptureConfig, available: &[CameraFormat]) -> Vec<CameraFormat> {
    let mut ordered = Vec::new();
    for wanted in wanted_formats(config.format) {
        let mut matching: Vec<CameraFormat> = available
            .iter()
            .filter(|format| format.format() == *wanted)
            .copied()
            .collect();
        if matching.is_empty() {
            continue;
        }
        matching.sort_by_key(|format| {
            (
                resolution_distance(format.resolution(), config.width, config.height),
                format.frame_rate().abs_diff(config.fps),
                std::cmp::Reverse(format.width() * format.height()),
                std::cmp::Reverse(format.frame_rate()),
            )
        });
        matching.dedup();
        ordered.extend(matching);
    }
    ordered
}

#[cfg(target_os = "windows")]
fn resolution_distance(resolution: nokhwa::utils::Resolution, width: u32, height: u32) -> u64 {
    let dx = u64::from(resolution.width().abs_diff(width));
    let dy = u64::from(resolution.height().abs_diff(height));
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
}

#[cfg(target_os = "windows")]
fn describe_formats(formats: &[CameraFormat]) -> String {
    if formats.is_empty() {
        return "none reported".to_owned();
    }
    formats
        .iter()
        .map(|format| {
            format!(
                "{} {}x{}@{}",
                format.format(),
                format.width(),
                format.height(),
                format.frame_rate()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(target_os = "macos")]
fn nominal_format(preferred: PreferredFormat) -> FrameFormat {
    match preferred {
        PreferredFormat::Gray => FrameFormat::GRAY,
        PreferredFormat::Yuyv => FrameFormat::YUYV,
        PreferredFormat::Mjpeg | PreferredFormat::Auto => FrameFormat::MJPEG,
    }
}

fn wanted_formats(preferred: PreferredFormat) -> &'static [FrameFormat] {
    match preferred {
        PreferredFormat::Auto => &[FrameFormat::GRAY, FrameFormat::YUYV, FrameFormat::MJPEG],
        PreferredFormat::Gray => &[FrameFormat::GRAY],
        PreferredFormat::Yuyv => &[FrameFormat::YUYV],
        PreferredFormat::Mjpeg => &[FrameFormat::MJPEG],
    }
}

fn info_from_format(device: &str, format: CameraFormat) -> Result<CameraInfo, CameraError> {
    let pixel_format = match format.format() {
        FrameFormat::GRAY => PixelFormat::Gray,
        FrameFormat::YUYV => PixelFormat::Yuyv,
        FrameFormat::MJPEG => PixelFormat::Mjpeg,
        other => return Err(CameraError::UnsupportedFormat(other.to_string())),
    };
    let width = format.width();
    let height = format.height();
    let stride = match pixel_format {
        PixelFormat::Gray => width,
        PixelFormat::Yuyv => width * 2,
        PixelFormat::Mjpeg => 0,
    };

    Ok(CameraInfo {
        device: device.to_owned(),
        name: "nokhwa camera".into(),
        width,
        height,
        stride,
        fps: Some(format.frame_rate() as f32),
        format: pixel_format,
    })
}

#[cfg(target_os = "macos")]
fn ensure_permission() -> Result<(), CameraError> {
    if nokhwa::nokhwa_check() {
        return Ok(());
    }

    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    nokhwa::nokhwa_initialize(move |granted| {
        let _ = sender.send(granted);
    });
    match receiver.recv_timeout(std::time::Duration::from_secs(15)) {
        Ok(true) => Ok(()),
        Ok(false) => Err(CameraError::PermissionDenied),
        Err(_) => Err(CameraError::PermissionTimeout),
    }
}

struct NokhwaSource {
    camera: NokhwaCamera,
    info: CameraInfo,
    counters: Arc<Counters>,
}

impl CameraSource for NokhwaSource {
    fn next_frame(&mut self) -> Result<Option<GrayFrame>, CameraError> {
        let buffer = self.camera.frame().map_err(|error| CameraError::Read {
            device: self.info.device.clone(),
            message: error.to_string(),
        })?;
        let resolution = buffer.resolution();
        let width = resolution.width();
        let height = resolution.height();

        let converted = match buffer.source_frame_format() {
            FrameFormat::GRAY => gray_to_gray(buffer.buffer(), width, height, width),
            FrameFormat::YUYV => yuyv_to_gray(buffer.buffer(), width, height, width * 2),
            FrameFormat::MJPEG => mjpeg_to_gray(buffer.buffer()),
            other => return Err(CameraError::UnsupportedFormat(other.to_string())),
        };

        match converted {
            Ok(frame) if !super::is_blank(&frame) => Ok(Some(frame)),
            Ok(_) | Err(_) => {
                self.counters.corrupt();
                Ok(None)
            }
        }
    }

    fn info(&self) -> &CameraInfo {
        &self.info
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use nokhwa::utils::Resolution;

    use super::*;

    fn format(width: u32, height: u32, format: FrameFormat, fps: u32) -> CameraFormat {
        CameraFormat::new(Resolution::new(width, height), format, fps)
    }

    #[test]
    fn auto_falls_back_from_a_missing_gray_format() {
        let available = [
            format(640, 480, FrameFormat::MJPEG, 30),
            format(640, 480, FrameFormat::YUYV, 30),
            format(1280, 720, FrameFormat::NV12, 30),
        ];
        let config = CaptureConfig {
            width: 640,
            height: 480,
            fps: 30,
            format: PreferredFormat::Auto,
            ..CaptureConfig::default()
        };

        let ordered = candidates(&config, &available);

        assert_eq!(ordered.first().unwrap().format(), FrameFormat::YUYV);
        assert_eq!(ordered.len(), 2);
    }

    #[test]
    fn a_forced_format_is_never_replaced() {
        let available = [
            format(640, 480, FrameFormat::MJPEG, 30),
            format(640, 480, FrameFormat::YUYV, 30),
        ];
        let config = CaptureConfig {
            width: 640,
            height: 480,
            fps: 30,
            format: PreferredFormat::Gray,
            ..CaptureConfig::default()
        };

        assert!(candidates(&config, &available).is_empty());
    }

    #[test]
    fn the_nearest_resolution_wins_before_the_nearest_frame_rate() {
        let available = [
            format(320, 240, FrameFormat::YUYV, 30),
            format(640, 360, FrameFormat::YUYV, 15),
            format(1280, 720, FrameFormat::YUYV, 30),
        ];
        let config = CaptureConfig {
            width: 640,
            height: 480,
            fps: 30,
            format: PreferredFormat::Yuyv,
            ..CaptureConfig::default()
        };

        let ordered = candidates(&config, &available);

        assert_eq!((ordered[0].width(), ordered[0].height()), (640, 360));
        assert_eq!(ordered[0].frame_rate(), 15);
        assert_eq!((ordered[1].width(), ordered[1].height()), (320, 240));
    }

    #[test]
    fn duplicate_media_types_are_reported_once() {
        let available = [
            format(640, 480, FrameFormat::YUYV, 30),
            format(640, 480, FrameFormat::YUYV, 30),
        ];
        let config = CaptureConfig {
            width: 640,
            height: 480,
            fps: 30,
            format: PreferredFormat::Yuyv,
            ..CaptureConfig::default()
        };

        assert_eq!(candidates(&config, &available).len(), 1);
    }

    #[test]
    fn an_unsupported_device_lists_what_it_offers() {
        let available = [
            format(640, 480, FrameFormat::NV12, 30),
            format(1280, 720, FrameFormat::NV12, 30),
        ];

        assert_eq!(
            describe_formats(&available),
            "NV12 640x480@30, NV12 1280x720@30"
        );
    }
}

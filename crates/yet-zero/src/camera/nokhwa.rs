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

use nokhwa::utils::{
    CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution,
};
use nokhwa::Camera as NokhwaCamera;

use yet_core::GrayFrame;

use super::convert::{gray_to_gray, mjpeg_to_gray, yuyv_to_gray};
use super::{
    CameraError, CameraInfo, CameraSource, CaptureConfig, Counters, PixelFormat, PreferredFormat,
};

pub(super) fn open(
    config: &CaptureConfig,
    counters: Arc<Counters>,
) -> Result<Box<dyn CameraSource>, CameraError> {
    #[cfg(target_os = "macos")]
    ensure_permission()?;

    let nominal = CameraFormat::new(
        Resolution::new(config.width, config.height),
        nominal_format(config.format),
        config.fps,
    );
    let requested = RequestedFormat::with_formats(
        RequestedFormatType::Closest(nominal),
        wanted_formats(config.format),
    );

    let device = format!("camera {}", config.device);
    let mut camera = NokhwaCamera::new(CameraIndex::Index(config.device as u32), requested)
        .map_err(|error| CameraError::Open {
            device: device.clone(),
            message: error.to_string(),
        })?;
    camera.open_stream().map_err(|error| CameraError::Open {
        device: device.clone(),
        message: error.to_string(),
    })?;

    let format = camera.camera_format();
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

    Ok(Box::new(NokhwaSource {
        camera,
        info: CameraInfo {
            device,
            name: "nokhwa camera".into(),
            width,
            height,
            stride,
            fps: Some(format.frame_rate() as f32),
            format: pixel_format,
        },
        counters,
    }))
}

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

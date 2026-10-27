// SPDX-License-Identifier: GPL-3.0-only

//! Linux camera adapter using `linuxvideo`.
//!
//! `linuxvideo` talks to V4L2 through `libc` only: no bindgen, no libclang and
//! no C compiler. The adapter opens the device inside the capture thread, so
//! the stream is created, read and dropped on one thread.

use std::sync::Arc;

use linuxvideo::format::{PixFormat, PixelFormat as V4l2PixelFormat};
use linuxvideo::{BufType, Device, Fract};

use yet_core::GrayFrame;

use super::convert::{gray_to_gray, mjpeg_to_gray, yuyv_to_gray};
use super::{
    CameraError, CameraInfo, CameraSource, CaptureConfig, Counters, PixelFormat, PreferredFormat,
};

pub(super) fn open(
    config: &CaptureConfig,
    counters: Arc<Counters>,
) -> Result<Box<dyn CameraSource>, CameraError> {
    let device_path = format!("/dev/video{}", config.device);
    let probe = Device::open(&device_path).map_err(|error| CameraError::Open {
        device: device_path.clone(),
        message: error.to_string(),
    })?;
    let name = probe
        .capabilities()
        .map(|capabilities| capabilities.card().to_owned())
        .unwrap_or_default();

    let mut available = Vec::new();
    for format in probe.formats(BufType::VIDEO_CAPTURE).flatten() {
        available.push(format.pixel_format());
    }
    drop(probe);

    let candidates = candidates(config.format, &available);
    if candidates.is_empty() {
        return Err(CameraError::NoFormat {
            device: device_path,
            available: available
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        });
    }

    let mut last_error = None;
    for format in candidates {
        match open_with(&device_path, &name, config, format, Arc::clone(&counters)) {
            Ok(camera) => return Ok(Box::new(camera)),
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error.unwrap_or_else(|| CameraError::NoFormat {
        device: device_path,
        available: String::new(),
    }))
}

fn open_with(
    device_path: &str,
    name: &str,
    config: &CaptureConfig,
    format: V4l2PixelFormat,
    counters: Arc<Counters>,
) -> Result<LinuxCamera, CameraError> {
    let device = Device::open(device_path).map_err(|error| CameraError::Open {
        device: device_path.to_owned(),
        message: error.to_string(),
    })?;
    let capture = device
        .video_capture(PixFormat::new(config.width, config.height, format))
        .map_err(|error| CameraError::Open {
            device: device_path.to_owned(),
            message: error.to_string(),
        })?;

    let negotiated = capture.format();
    let width = negotiated.width();
    let height = negotiated.height();
    let stride = negotiated.bytes_per_line();
    let pixel_format = to_pixel_format(negotiated.pixel_format())?;
    let fps = capture
        .set_frame_interval(Fract::new(1, config.fps))
        .ok()
        .and_then(|interval| {
            let numerator = interval.numerator();
            let denominator = interval.denominator();
            (numerator != 0).then_some(denominator as f32 / numerator as f32)
        });

    let stream = capture.into_stream().map_err(|error| CameraError::Open {
        device: device_path.to_owned(),
        message: error.to_string(),
    })?;

    Ok(LinuxCamera {
        stream,
        info: CameraInfo {
            device: device_path.to_owned(),
            name: name.to_owned(),
            width,
            height,
            stride,
            fps,
            format: pixel_format,
        },
        counters,
    })
}

fn candidates(requested: PreferredFormat, available: &[V4l2PixelFormat]) -> Vec<V4l2PixelFormat> {
    let mut ordered = Vec::new();
    let mut push = |format: V4l2PixelFormat| {
        if available.contains(&format) && !ordered.contains(&format) {
            ordered.push(format);
        }
    };

    match requested {
        PreferredFormat::Auto => {
            for format in grey_formats() {
                push(format);
            }
            push(V4l2PixelFormat::YUYV);
            push(V4l2PixelFormat::MJPG);
            push(V4l2PixelFormat::JPEG);
        }
        PreferredFormat::Gray => {
            for format in grey_formats() {
                push(format);
            }
        }
        PreferredFormat::Yuyv => push(V4l2PixelFormat::YUYV),
        PreferredFormat::Mjpeg => {
            push(V4l2PixelFormat::MJPG);
            push(V4l2PixelFormat::JPEG);
        }
    }

    ordered
}

fn grey_formats() -> [V4l2PixelFormat; 3] {
    [
        V4l2PixelFormat::from_fourcc(*b"GREY"),
        V4l2PixelFormat::from_fourcc(*b"Y800"),
        V4l2PixelFormat::from_fourcc(*b"Y8  "),
    ]
}

fn to_pixel_format(format: V4l2PixelFormat) -> Result<PixelFormat, CameraError> {
    if format == V4l2PixelFormat::YUYV {
        Ok(PixelFormat::Yuyv)
    } else if format == V4l2PixelFormat::MJPG || format == V4l2PixelFormat::JPEG {
        Ok(PixelFormat::Mjpeg)
    } else if grey_formats().contains(&format) {
        Ok(PixelFormat::Gray)
    } else {
        Err(CameraError::UnsupportedFormat(format.to_string()))
    }
}

struct LinuxCamera {
    stream: linuxvideo::stream::ReadStream,
    info: CameraInfo,
    counters: Arc<Counters>,
}

impl CameraSource for LinuxCamera {
    fn next_frame(&mut self) -> Result<Option<GrayFrame>, CameraError> {
        let Self {
            stream,
            info,
            counters,
        } = self;

        let result = stream.dequeue(|view| {
            if view.is_error() {
                counters.corrupt();
                return Ok(None);
            }

            let bytes: &[u8] = &view;
            let converted = match info.format {
                PixelFormat::Gray => gray_to_gray(bytes, info.width, info.height, info.stride),
                PixelFormat::Yuyv => yuyv_to_gray(bytes, info.width, info.height, info.stride),
                PixelFormat::Mjpeg => mjpeg_to_gray(bytes),
            };

            match converted {
                Ok(frame) if !super::is_blank(&frame) => Ok(Some(frame)),
                Ok(_) | Err(_) => {
                    counters.corrupt();
                    Ok(None)
                }
            }
        });

        result.map_err(|error| CameraError::Read {
            device: info.device.clone(),
            message: error.to_string(),
        })
    }

    fn info(&self) -> &CameraInfo {
        &self.info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_preference_orders_gray_then_yuyv_then_mjpeg() {
        let available = [
            V4l2PixelFormat::MJPG,
            V4l2PixelFormat::YUYV,
            V4l2PixelFormat::from_fourcc(*b"GREY"),
        ];

        assert_eq!(
            candidates(PreferredFormat::Auto, &available),
            vec![
                V4l2PixelFormat::from_fourcc(*b"GREY"),
                V4l2PixelFormat::YUYV,
                V4l2PixelFormat::MJPG,
            ]
        );
    }

    #[test]
    fn a_forced_format_is_not_replaced() {
        let available = [V4l2PixelFormat::MJPG, V4l2PixelFormat::YUYV];

        assert_eq!(
            candidates(PreferredFormat::Mjpeg, &available),
            vec![V4l2PixelFormat::MJPG]
        );
        assert!(candidates(PreferredFormat::Gray, &available).is_empty());
    }
}

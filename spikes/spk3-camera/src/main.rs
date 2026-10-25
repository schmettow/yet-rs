// SPDX-License-Identifier: GPL-3.0-only
//! SPK-3: camera capture and quadrant-feature parity.
//!
//! On Linux this uses `linuxvideo` and captures from `/dev/video0` (or the
//! device in `SPK3_DEVICE`). It lists formats, grabs 30 frames, computes the
//! four quadrant means over a fixed ROI, and writes `spk3-capture.png` and
//! `spk3-quad.csv`.
//!
//! With `--fixture <png>` it computes the same features for a fixture so the
//! Python reference in `parity.py` can be compared.

use std::env;
use std::error::Error;

#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::io::{BufWriter, Write};

mod features;

use features::{quad_brightness, EyeRegion, GrayFrame, QuadBrightness};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() == 3 && args[1] == "--fixture" {
        return fixture_mode(&args[2]);
    }
    capture_mode()
}

fn fixture_mode(path: &str) -> Result<(), Box<dyn Error>> {
    let rgb = image::open(path)?.to_rgb8();
    let (width, height) = rgb.dimensions();
    let data = rgb.pixels().map(|pixel| luma_601(pixel.0)).collect();
    let frame = GrayFrame {
        width,
        height,
        data,
    };
    let roi = EyeRegion {
        x: 0,
        y: 0,
        width: frame.width,
        height: frame.height,
    };
    print_quadrants(quad_brightness(&frame, roi));
    Ok(())
}

/// ITU-R BT.601 luma, matching OpenCV `BGR2GRAY` and PIL `convert("L")`.
///
/// Do not use `image::DynamicImage::to_luma8()` here: it uses Rec. 709
/// coefficients, which produced 54/182/18 for the red/green/blue fixture
/// instead of the expected 76/150/29.
fn luma_601([r, g, b]: [u8; 3]) -> u8 {
    let value = u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114;
    u8::try_from((value + 500) / 1000).unwrap_or(u8::MAX)
}

#[cfg(target_os = "linux")]
fn capture_mode() -> Result<(), Box<dyn Error>> {
    use std::time::Instant;

    use linuxvideo::format::{FormatDesc, PixFormat};
    use linuxvideo::{BufType, Device};

    let path = env::var("SPK3_DEVICE").unwrap_or_else(|_| "/dev/video0".into());

    let probe = Device::open(&path)?;
    let capabilities = probe.capabilities()?;
    println!(
        "device={path} driver={} card={} bus={}",
        capabilities.driver(),
        capabilities.card(),
        capabilities.bus_info()
    );

    let mut available = Vec::new();
    for format in probe.formats(BufType::VIDEO_CAPTURE) {
        let format: FormatDesc = format?;
        println!(
            "format={} description={}",
            format.pixel_format(),
            format.description()
        );
        available.push(format.pixel_format());
    }
    drop(probe);

    let chosen = if let Ok(requested) = env::var("SPK3_FORMAT") {
        available
            .iter()
            .copied()
            .find(|format| format.to_string() == requested)
            .ok_or_else(|| format!("requested format {requested} is not available"))?
    } else {
        choose_format(&available).ok_or("no supported capture format")?
    };
    let device = Device::open(&path)?;
    let capture = device.video_capture(PixFormat::new(640, 480, chosen))?;
    let negotiated = capture.format();
    let info = FormatInfo {
        width: negotiated.width(),
        height: negotiated.height(),
        stride: negotiated.bytes_per_line(),
        pixel_format: negotiated.pixel_format(),
    };
    println!(
        "negotiated={} {}x{} stride={}",
        info.pixel_format, info.width, info.height, info.stride
    );
    if let Ok(interval) = capture.set_frame_interval(linuxvideo::Fract::new(1, 30)) {
        println!(
            "frame_interval={}/{}",
            interval.numerator(),
            interval.denominator()
        );
    }

    let mut stream = capture.into_stream()?;
    let mut rows = BufWriter::new(File::create("spk3-quad.csv")?);
    writeln!(rows, "frame,nw,ne,sw,se")?;

    let roi = EyeRegion {
        x: 160,
        y: 120,
        width: 320,
        height: 240,
    };
    let mut saved = false;
    let mut count = 0_u32;
    let mut corrupt = 0_u32;
    let mut started: Option<Instant> = None;

    while count < 30 {
        stream.dequeue(|frame| {
            if frame.is_error() {
                corrupt += 1;
                return Ok(());
            }
            let gray = frame_to_gray(&frame, info)?;
            let quad = quad_brightness(
                &gray,
                EyeRegion {
                    x: 0,
                    y: 0,
                    width: gray.width,
                    height: gray.height,
                },
            );
            writeln!(
                rows,
                "{count},{},{},{},{}",
                quad.nw, quad.ne, quad.sw, quad.se
            )?;
            if !saved {
                save_gray(&gray, "spk3-capture.png")?;
                saved = true;
            }
            if started.is_none() {
                started = Some(Instant::now());
            }
            count += 1;
            Ok(())
        })?;
    }
    rows.flush()?;

    let elapsed = started
        .map(|start| start.elapsed().as_secs_f64())
        .unwrap_or(0.0);
    let fps = if count > 1 && elapsed > 0.0 {
        f64::from(count - 1) / elapsed
    } else {
        0.0
    };
    println!(
        "captured={count} corrupt={corrupt} elapsed_s={elapsed:.3} fps={fps:.2} roi={roi:?} png=spk3-capture.png csv=spk3-quad.csv"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct FormatInfo {
    width: u32,
    height: u32,
    stride: u32,
    pixel_format: linuxvideo::format::PixelFormat,
}

#[cfg(target_os = "linux")]
fn choose_format(
    available: &[linuxvideo::format::PixelFormat],
) -> Option<linuxvideo::format::PixelFormat> {
    use linuxvideo::format::PixelFormat;

    let grey = [
        PixelFormat::from_fourcc(*b"GREY"),
        PixelFormat::from_fourcc(*b"Y800"),
        PixelFormat::from_fourcc(*b"Y8  "),
    ];
    available
        .iter()
        .copied()
        .find(|format| *format == PixelFormat::YUYV)
        .or_else(|| {
            available
                .iter()
                .copied()
                .find(|format| grey.contains(format))
        })
        .or_else(|| {
            available
                .iter()
                .copied()
                .find(|format| *format == PixelFormat::JPEG)
        })
        .or_else(|| {
            available
                .iter()
                .copied()
                .find(|format| *format == PixelFormat::MJPG)
        })
        .or_else(|| available.first().copied())
}

#[cfg(target_os = "linux")]
fn frame_to_gray(
    frame: &linuxvideo::stream::ReadBufferView<'_>,
    info: FormatInfo,
) -> std::io::Result<GrayFrame> {
    use linuxvideo::format::PixelFormat;

    let width = info.width;
    let height = info.height;
    let stride = info.stride.max(width * 2);
    let bytes: &[u8] = frame;
    let mut data = Vec::with_capacity((width * height) as usize);

    let grey = [
        PixelFormat::from_fourcc(*b"GREY"),
        PixelFormat::from_fourcc(*b"Y800"),
        PixelFormat::from_fourcc(*b"Y8  "),
    ];

    if info.pixel_format == PixelFormat::YUYV {
        for y in 0..height {
            let row = y as usize * stride as usize;
            for x in 0..width {
                let index = row + x as usize * 2;
                data.push(
                    *bytes
                        .get(index)
                        .ok_or_else(|| std::io::Error::other("YUYV frame is truncated"))?,
                );
            }
        }
    } else if grey.contains(&info.pixel_format) {
        for y in 0..height {
            let row = y as usize * info.stride.max(width) as usize;
            let end = row + width as usize;
            data.extend_from_slice(
                bytes
                    .get(row..end)
                    .ok_or_else(|| std::io::Error::other("gray frame is truncated"))?,
            );
        }
    } else if info.pixel_format == PixelFormat::JPEG || info.pixel_format == PixelFormat::MJPG {
        let decoded = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg)
            .map_err(std::io::Error::other)?
            .to_rgb8();
        let (width, height) = decoded.dimensions();
        let data = decoded.pixels().map(|pixel| luma_601(pixel.0)).collect();
        return Ok(GrayFrame {
            width,
            height,
            data,
        });
    } else {
        return Err(std::io::Error::other(format!(
            "unsupported pixel format: {}",
            info.pixel_format
        )));
    }

    Ok(GrayFrame {
        width,
        height,
        data,
    })
}

#[cfg(target_os = "linux")]
fn save_gray(frame: &GrayFrame, path: &str) -> std::io::Result<()> {
    let image = image::GrayImage::from_raw(frame.width, frame.height, frame.data.clone())
        .ok_or_else(|| std::io::Error::other("invalid gray frame"))?;
    image.save(path).map_err(std::io::Error::other)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn capture_mode() -> Result<(), Box<dyn Error>> {
    use nokhwa::pixel_format::RgbFormat;
    use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};
    use nokhwa::Camera;

    let requested =
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
    let mut camera = Camera::new(CameraIndex::Index(0), requested)?;
    println!(
        "camera={} format={:?}",
        camera.info().human_name(),
        camera.camera_format()
    );
    let frame = camera.frame()?;
    println!(
        "frame={}x{} bytes={}",
        frame.resolution().width(),
        frame.resolution().height(),
        frame.buffer().len()
    );
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn capture_mode() -> Result<(), Box<dyn Error>> {
    println!("camera capture is not implemented for this platform");
    Ok(())
}

fn print_quadrants(quad: QuadBrightness) {
    println!(
        "{{\"nw\":{},\"ne\":{},\"sw\":{},\"se\":{}}}",
        quad.nw, quad.ne, quad.sw, quad.se
    );
}

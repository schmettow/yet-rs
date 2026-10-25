// SPDX-License-Identifier: GPL-3.0-only

use yet_core::camera::DEFAULT_EYE_REGION;
use yet_core::{
    validate_eye_region, Camera, CameraError, CameraInfo, CaptureConfig, EyeRegion, GrayFrame,
    PixelFormat,
};

fn info(width: u32, height: u32) -> CameraInfo {
    CameraInfo {
        device: "/dev/video0".into(),
        name: "test camera".into(),
        width,
        height,
        stride: width,
        fps: Some(30.0),
        format: PixelFormat::Gray,
    }
}

#[test]
fn camera_protocol_types_are_send() {
    fn assert_send<T: Send>() {}

    assert_send::<Camera>();
    assert_send::<CaptureConfig>();
    assert_send::<CameraInfo>();
    assert_send::<CameraError>();
    assert_send::<GrayFrame>();
}

#[test]
fn the_default_eye_region_is_the_centered_half() {
    assert_eq!(
        info(640, 480).eye_region(DEFAULT_EYE_REGION),
        Some(EyeRegion::new(160, 120, 320, 240))
    );
}

#[test]
fn an_odd_resolution_keeps_the_rounded_region_inside_the_frame() {
    assert_eq!(
        info(641, 481).eye_region(DEFAULT_EYE_REGION),
        Some(EyeRegion::new(160, 120, 321, 241))
    );
}

#[test]
fn invalid_eye_regions_are_rejected() {
    assert!(!validate_eye_region([0.6, 0.6, 0.5, 0.5]));
    assert!(!validate_eye_region([0.0, 0.0, 0.0, 0.5]));
    assert!(!validate_eye_region([0.0, 0.0, 0.5, 0.0]));
    assert!(!validate_eye_region([f32::NAN, 0.0, 0.5, 0.5]));
    assert_eq!(info(640, 480).eye_region([0.6, 0.6, 0.5, 0.5]), None);
    assert_eq!(info(640, 480).eye_region([0.0, 0.0, 0.0, 0.5]), None);
}

#[test]
fn a_region_that_rounds_to_one_pixel_is_rejected() {
    assert_eq!(info(640, 480).eye_region([0.0, 0.0, 0.001, 0.001]), None);
}

#[test]
fn capture_configuration_is_validated() {
    assert_eq!(
        CaptureConfig {
            width: 0,
            ..CaptureConfig::default()
        }
        .validate(),
        Err(CameraError::InvalidDimensions {
            width: 0,
            height: 480,
        })
    );
    assert_eq!(
        CaptureConfig {
            fps: 0,
            ..CaptureConfig::default()
        }
        .validate(),
        Err(CameraError::Config("camera fps must be positive".into()))
    );
    assert_eq!(CaptureConfig::default().validate(), Ok(()));
}

#[cfg(target_os = "linux")]
#[test]
fn a_missing_camera_reports_an_open_error() {
    let error = Camera::open(CaptureConfig {
        device: 999,
        ..CaptureConfig::default()
    })
    .unwrap_err();

    assert!(matches!(error, CameraError::Open { .. }), "{error:?}");
}

#[test]
#[ignore = "requires a real camera; run manually for the SPK-5 smoke test"]
fn a_real_camera_delivers_a_frame() {
    use std::thread;
    use std::time::{Duration, Instant};

    let mut camera = Camera::open(CaptureConfig::default()).expect("camera");
    println!("info: {:?}", camera.info());

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut frame = None;
    while Instant::now() < deadline && frame.is_none() {
        frame = camera.latest_frame();
        thread::sleep(Duration::from_millis(10));
    }

    println!("stats: {:?}", camera.stats());
    let frame = frame.expect("a frame within two seconds");
    assert_eq!(
        (frame.width(), frame.height()),
        (camera.info().width, camera.info().height)
    );
    camera.stop();
}

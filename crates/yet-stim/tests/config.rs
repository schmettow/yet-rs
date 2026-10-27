// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;
use yet_stim::{load_config, ConfigError};

const VALID: &str = r#"
[display]
presenter_monitor = 1
console_monitor = 0
width = 1920
height = 1080
fullscreen = true

[camera]
usb_index = 0

[presenter]
backend = "egui"
overlay = "auto"

[overlay]
monitor = 1

[audio]
enabled = true
device = "default"
volume = 0.8
"#;

fn write_config(text: &str) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, text).unwrap();
    (dir, path)
}

#[test]
fn valid_config_loads_and_validates() {
    let (_dir, path) = write_config(VALID);
    let config = load_config(&path).unwrap();

    assert_eq!(config.display.presenter_monitor, 1);
    assert_eq!(config.display.console_monitor, 0);
    assert_eq!(config.camera.usb_index, 0);
    assert_eq!(config.camera.width, 640);
    assert_eq!(config.camera.height, 480);
    assert_eq!(config.camera.fps, 30);
    assert_eq!(config.camera.format, yet_stim::PreferredFormat::Auto);
    assert_eq!(config.camera.eye_region, None);
    assert_eq!(config.presenter.overlay, yet_stim::OverlayMode::Auto);
    assert_eq!(config.overlay.monitor, Some(1));
    assert_eq!(config.audio.volume, 0.8);
    assert_eq!(config.audio.output_latency_ms, 0);
    config.validate_monitors(2).unwrap();
}

#[test]
fn defaults_fill_optional_sections() {
    let text = r#"
[display]
presenter_monitor = 0
console_monitor = 0
width = 800
height = 600
fullscreen = false
"#;
    let (_dir, path) = write_config(text);
    let config = load_config(&path).unwrap();

    assert_eq!(config.camera.usb_index, 0);
    assert_eq!(config.camera.width, 640);
    assert_eq!(config.camera.height, 480);
    assert_eq!(config.camera.fps, 30);
    assert_eq!(config.camera.format, yet_stim::PreferredFormat::Auto);
    assert_eq!(config.camera.eye_region, None);
    assert_eq!(config.presenter.backend, yet_stim::PresenterBackend::Egui);
    assert_eq!(config.presenter.overlay, yet_stim::OverlayMode::Auto);
    assert_eq!(config.overlay.monitor, None);
    assert!(config.audio.enabled);
    assert_eq!(config.audio.device, "default");
    assert_eq!(config.audio.volume, 1.0);
}

#[test]
fn camera_settings_load() {
    let text = VALID.replace(
        "usb_index = 0",
        "usb_index = 1\nwidth = 1280\nheight = 720\nfps = 60\nformat = \"mjpeg\"\neye_region = [0.2, 0.2, 0.6, 0.6]",
    );
    let (_dir, path) = write_config(&text);
    let config = load_config(&path).unwrap();

    assert_eq!(config.camera.usb_index, 1);
    assert_eq!((config.camera.width, config.camera.height), (1280, 720));
    assert_eq!(config.camera.fps, 60);
    assert_eq!(config.camera.format, yet_stim::PreferredFormat::Mjpeg);
    assert_eq!(config.camera.eye_region, Some([0.2, 0.2, 0.6, 0.6]));
}

#[test]
fn an_out_of_bounds_eye_region_is_rejected() {
    let (_dir, path) = write_config(&VALID.replace(
        "usb_index = 0",
        "usb_index = 0\neye_region = [0.6, 0.6, 0.5, 0.5]",
    ));

    assert!(matches!(
        load_config(&path).unwrap_err(),
        ConfigError::InvalidEyeRegion { .. }
    ));
}

#[test]
fn zero_camera_fps_is_rejected() {
    let (_dir, path) = write_config(&VALID.replace("usb_index = 0", "usb_index = 0\nfps = 0"));

    assert!(matches!(
        load_config(&path).unwrap_err(),
        ConfigError::InvalidCameraFps { fps: 0 }
    ));
}

#[test]
fn zero_camera_resolution_is_rejected() {
    let (_dir, path) = write_config(&VALID.replace("usb_index = 0", "usb_index = 0\nwidth = 0"));

    assert!(matches!(
        load_config(&path).unwrap_err(),
        ConfigError::InvalidCameraResolution { width: 0, .. }
    ));
}

#[test]
fn an_unknown_camera_format_is_rejected() {
    let (_dir, path) =
        write_config(&VALID.replace("usb_index = 0", "usb_index = 0\nformat = \"bogus\""));

    assert!(matches!(
        load_config(&path).unwrap_err(),
        ConfigError::Toml { .. }
    ));
}

#[test]
fn zero_resolution_is_rejected() {
    let (_dir, path) = write_config(&VALID.replace("width = 1920", "width = 0"));
    let error = load_config(&path).unwrap_err();

    assert!(matches!(
        error,
        ConfigError::InvalidResolution { width: 0, .. }
    ));
}

#[test]
fn out_of_range_volume_is_rejected() {
    let (_dir, path) = write_config(&VALID.replace("volume = 0.8", "volume = 1.5"));
    let error = load_config(&path).unwrap_err();

    match error {
        ConfigError::InvalidVolume { volume } => assert!((volume - 1.5).abs() < f32::EPSILON),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn empty_audio_device_is_rejected() {
    let (_dir, path) = write_config(&VALID.replace("device = \"default\"", "device = \"  \""));
    let error = load_config(&path).unwrap_err();

    assert!(matches!(error, ConfigError::EmptyAudioDevice));
}

#[test]
fn monitor_index_is_validated_against_available_monitors() {
    let (_dir, path) =
        write_config(&VALID.replace("presenter_monitor = 1", "presenter_monitor = 5"));
    let config = load_config(&path).unwrap();
    let error = config.validate_monitors(2).unwrap_err();

    assert!(matches!(
        error,
        ConfigError::InvalidMonitor {
            field: "display.presenter_monitor",
            index: 5,
            available: 2,
        }
    ));
}

#[test]
fn an_implausible_audio_latency_is_rejected() {
    let (_dir, path) =
        write_config(&VALID.replace("volume = 0.8", "volume = 0.8\noutput_latency_ms = 5000"));
    let error = load_config(&path).unwrap_err();

    assert!(matches!(
        error,
        ConfigError::InvalidAudioLatency { milliseconds: 5000 }
    ));
}

#[test]
fn unknown_overlay_is_rejected() {
    let (_dir, path) = write_config(&VALID.replace("overlay = \"auto\"", "overlay = \"bogus\""));
    let error = load_config(&path).unwrap_err();

    assert!(matches!(error, ConfigError::Toml { .. }));
}

#[test]
fn unknown_backend_is_rejected() {
    let (_dir, path) =
        write_config(&VALID.replace("backend = \"egui\"", "backend = \"macroquad\""));
    let error = load_config(&path).unwrap_err();

    assert!(matches!(error, ConfigError::Toml { .. }));
}

#[test]
fn unknown_display_field_is_rejected() {
    let (_dir, path) =
        write_config(&VALID.replace("fullscreen = true", "fullscreen = true\nscale = 1.5"));
    let error = load_config(&path).unwrap_err();

    assert!(matches!(error, ConfigError::Toml { .. }));
}

#[test]
fn missing_config_is_reported_with_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    let error = load_config(&path).unwrap_err();

    match error {
        ConfigError::Io {
            path: attempted, ..
        } => assert_eq!(attempted, path),
        other => panic!("unexpected error: {other:?}"),
    }
}

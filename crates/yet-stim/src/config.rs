// SPDX-License-Identifier: GPL-3.0-only

//! Machine configuration (`config.toml`).
//!
//! Experiment content belongs in CSV; this module only covers display, camera,
//! presenter, overlay and audio settings.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use yet_core::validate_eye_region;

/// Machine configuration for one experiment directory.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    /// Display and monitor settings.
    pub display: DisplayConfig,
    /// Camera settings.
    #[serde(default)]
    pub camera: CameraConfig,
    /// Presenter and overlay selection.
    #[serde(default)]
    pub presenter: PresenterConfig,
    /// Overlay monitor selection.
    #[serde(default)]
    pub overlay: OverlayConfig,
    /// Audio output settings.
    #[serde(default)]
    pub audio: AudioConfig,
}

/// Display and monitor settings.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayConfig {
    /// Zero-based monitor index for the stimulus window.
    pub presenter_monitor: usize,
    /// Zero-based monitor index for the console.
    pub console_monitor: usize,
    /// Requested surface width in physical pixels.
    pub width: u32,
    /// Requested surface height in physical pixels.
    pub height: u32,
    /// Whether the presenter uses borderless fullscreen.
    pub fullscreen: bool,
}

/// Camera settings.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraConfig {
    /// Zero-based USB camera index.
    #[serde(default)]
    pub usb_index: usize,
    /// Requested capture width.
    #[serde(default = "default_camera_width")]
    pub width: u32,
    /// Requested capture height.
    #[serde(default = "default_camera_height")]
    pub height: u32,
    /// Requested frame rate.
    #[serde(default = "default_camera_fps")]
    pub fps: u32,
    /// Format preference.
    #[serde(default)]
    pub format: PreferredFormat,
    /// Normalized eye region `[x, y, width, height]` in `0..=1`.
    #[serde(default)]
    pub eye_region: Option<[f32; 4]>,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            usb_index: 0,
            width: default_camera_width(),
            height: default_camera_height(),
            fps: default_camera_fps(),
            format: PreferredFormat::Auto,
            eye_region: None,
        }
    }
}

fn default_camera_width() -> u32 {
    640
}

fn default_camera_height() -> u32 {
    480
}

fn default_camera_fps() -> u32 {
    30
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

/// Presenter backend and overlay mode.
#[derive(Clone, Debug, PartialEq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PresenterConfig {
    /// Presenter implementation.
    #[serde(default)]
    pub backend: PresenterBackend,
    /// Overlay backend selection.
    #[serde(default)]
    pub overlay: OverlayMode,
}

/// Supported presenter backends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresenterBackend {
    /// egui/eframe in-process presenter.
    #[default]
    Egui,
}

impl fmt::Display for PresenterBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Egui => f.write_str("egui"),
        }
    }
}

/// Overlay backend selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayMode {
    /// Choose the platform backend automatically.
    #[default]
    Auto,
    /// Disable the overlay.
    Off,
    /// eframe overlay for Windows/macOS.
    Egui,
    /// Wayland layer-shell overlay for KDE, wlroots/Sway and naked compositors.
    LayerShell,
    /// XWayland overlay fallback.
    ///
    /// GNOME-on-Wayland overlay support is **not implemented**; selecting this
    /// returns a clear error that points to layer-shell or `off`.
    Xwayland,
}

impl fmt::Display for OverlayMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => f.write_str("auto"),
            Self::Off => f.write_str("off"),
            Self::Egui => f.write_str("egui"),
            Self::LayerShell => f.write_str("layer-shell"),
            Self::Xwayland => f.write_str("xwayland"),
        }
    }
}

/// Overlay monitor selection.
#[derive(Clone, Debug, PartialEq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct OverlayConfig {
    /// Monitor for the overlay; defaults to the presenter monitor.
    #[serde(default)]
    pub monitor: Option<usize>,
}

/// Audio output settings.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioConfig {
    /// Whether audio output is required for a session.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Output device name, or `default`.
    #[serde(default = "default_device")]
    pub device: String,
    /// Output volume in `0.0..=1.0`.
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Measured output-buffer latency added to the audio clock, in milliseconds.
    ///
    /// The default is 0: the integrated S8 run showed `get_pos()` tracking
    /// wall time, while SPK-4's −128 ms included stream startup. Measure the
    /// actual device with a beep/flash recording before setting this; values
    /// above ~50 ms should be treated as suspicious.
    #[serde(default = "default_audio_latency_ms")]
    pub output_latency_ms: u32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            device: default_device(),
            volume: default_volume(),
            output_latency_ms: default_audio_latency_ms(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_device() -> String {
    "default".to_owned()
}

fn default_volume() -> f32 {
    1.0
}

fn default_audio_latency_ms() -> u32 {
    0
}

/// Configuration loading and validation errors.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The file could not be read.
    #[error("could not read config `{path}`: {source}")]
    Io {
        /// Attempted path.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The TOML is invalid or does not match the schema.
    #[error("invalid TOML in `{path}`: {source}")]
    Toml {
        /// Attempted path.
        path: PathBuf,
        /// TOML parse error.
        #[source]
        source: Box<toml::de::Error>,
    },
    /// The requested resolution is empty.
    #[error("display resolution must be positive, got {width}x{height}")]
    InvalidResolution {
        /// Requested width.
        width: u32,
        /// Requested height.
        height: u32,
    },
    /// The volume is outside `0.0..=1.0`.
    #[error("audio volume must be between 0 and 1, got {volume}")]
    InvalidVolume {
        /// Requested volume.
        volume: f32,
    },
    /// The audio device name is empty.
    #[error("audio device name must not be empty")]
    EmptyAudioDevice,
    /// The audio latency is implausibly large.
    #[error("audio output latency must be at most 1000 ms, got {milliseconds} ms")]
    InvalidAudioLatency {
        /// Requested latency in milliseconds.
        milliseconds: u32,
    },
    /// The requested camera resolution is empty.
    #[error("camera resolution must be positive, got {width}x{height}")]
    InvalidCameraResolution {
        /// Requested width.
        width: u32,
        /// Requested height.
        height: u32,
    },
    /// The requested camera frame rate is zero.
    #[error("camera fps must be positive, got {fps}")]
    InvalidCameraFps {
        /// Requested frame rate.
        fps: u32,
    },
    /// The normalized eye region is outside the unit square.
    #[error(
        "camera eye region must be a normalized [x, y, width, height] inside 0..=1, got {region:?}"
    )]
    InvalidEyeRegion {
        /// Supplied region.
        region: [f32; 4],
    },
    /// No monitors are available for validation.
    #[error("no monitors are available")]
    NoMonitors,
    /// A configured monitor index does not exist.
    #[error(
        "monitor index for `{field}` is {index}, but only {available} monitor(s) are available"
    )]
    InvalidMonitor {
        /// Configuration field.
        field: &'static str,
        /// Requested index.
        index: usize,
        /// Number of available monitors.
        available: usize,
    },
}

impl AppConfig {
    /// Validate machine-independent settings.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.display.width == 0 || self.display.height == 0 {
            return Err(ConfigError::InvalidResolution {
                width: self.display.width,
                height: self.display.height,
            });
        }
        if self.camera.width == 0 || self.camera.height == 0 {
            return Err(ConfigError::InvalidCameraResolution {
                width: self.camera.width,
                height: self.camera.height,
            });
        }
        if self.camera.fps == 0 {
            return Err(ConfigError::InvalidCameraFps {
                fps: self.camera.fps,
            });
        }
        if let Some(region) = self.camera.eye_region {
            if !validate_eye_region(region) {
                return Err(ConfigError::InvalidEyeRegion { region });
            }
        }
        if !self.audio.volume.is_finite() || !(0.0..=1.0).contains(&self.audio.volume) {
            return Err(ConfigError::InvalidVolume {
                volume: self.audio.volume,
            });
        }
        if self.audio.device.trim().is_empty() {
            return Err(ConfigError::EmptyAudioDevice);
        }
        if self.audio.output_latency_ms > 1_000 {
            return Err(ConfigError::InvalidAudioLatency {
                milliseconds: self.audio.output_latency_ms,
            });
        }
        Ok(())
    }

    /// Validate monitor indices against the number of monitors reported by the platform.
    pub fn validate_monitors(&self, available: usize) -> Result<(), ConfigError> {
        if available == 0 {
            return Err(ConfigError::NoMonitors);
        }
        check_monitor(
            "display.presenter_monitor",
            self.display.presenter_monitor,
            available,
        )?;
        check_monitor(
            "display.console_monitor",
            self.display.console_monitor,
            available,
        )?;
        if let Some(monitor) = self.overlay.monitor {
            check_monitor("overlay.monitor", monitor, available)?;
        }
        Ok(())
    }
}

fn check_monitor(field: &'static str, index: usize, available: usize) -> Result<(), ConfigError> {
    if index >= available {
        Err(ConfigError::InvalidMonitor {
            field,
            index,
            available,
        })
    } else {
        Ok(())
    }
}

/// Load and validate a `config.toml`.
pub fn load(path: impl AsRef<Path>) -> Result<AppConfig, ConfigError> {
    let path = path.as_ref().to_path_buf();
    let text = fs::read_to_string(&path).map_err(|source| ConfigError::Io {
        path: path.clone(),
        source,
    })?;
    let config: AppConfig = toml::from_str(&text).map_err(|source| ConfigError::Toml {
        path: path.clone(),
        source: Box::new(source),
    })?;
    config.validate()?;
    Ok(config)
}

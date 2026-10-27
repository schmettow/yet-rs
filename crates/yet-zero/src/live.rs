// SPDX-License-Identifier: GPL-3.0-only

//! Live camera diagnostics and terminal-driven calibration.
//!
//! These functions are the Sprint 5 hardware path. The graphics console in
//! Sprint 7 will replace the terminal prompts with the presenter, but it will
//! use the same [`Calibration`] and [`CalibrationGrid`] types.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::camera::DEFAULT_EYE_REGION;
use crate::camera::{Camera, CameraError, CameraInfo, CameraStats, CaptureConfig};
use thiserror::Error;
use yet_core::{
    quad_brightness, Calibration, CalibrationReport, EyeRegion, GazePoint, GrayFrame, ModelError,
    QuadBrightness,
};
use yet_stim::{CalibrationGrid, LoadError};

/// Result of a camera probe.
#[derive(Clone, Debug)]
pub struct CameraProbe {
    /// Negotiated camera metadata.
    pub info: CameraInfo,
    /// Capture counters.
    pub stats: CameraStats,
    /// Probe duration.
    pub elapsed: Duration,
    /// Frames taken from the mailbox by the consumer.
    pub consumed: u64,
    /// Average frame rate reported by the capture thread.
    pub captured_fps: f64,
    /// Last quadrant means for the configured region.
    pub quadrants: Option<QuadBrightness>,
    /// Eye region used for the quadrant means and the optional dump.
    pub roi: EyeRegion,
    /// Path of the raw ROI dump, when requested.
    pub saved_roi: Option<PathBuf>,
}

/// Options for [`calibrate`].
#[derive(Clone, Copy, Debug)]
pub struct CalibrateOptions {
    /// Frames averaged per calibration target.
    pub samples: usize,
    /// Run a center-target recalibration after the full grid.
    pub quick: bool,
    /// Skip the live gaze printout.
    pub skip_live: bool,
    /// Duration of the live gaze printout.
    pub live_seconds: f64,
    /// Override the configured camera index.
    pub device: Option<usize>,
    /// Use the terminal instead of the fullscreen window.
    pub terminal: bool,
}

impl Default for CalibrateOptions {
    fn default() -> Self {
        Self {
            samples: 20,
            quick: false,
            skip_live: false,
            live_seconds: 5.0,
            device: None,
            terminal: false,
        }
    }
}

/// Result of [`calibrate`].
#[derive(Clone, Debug)]
pub struct CalibrationOutcome {
    /// Negotiated camera metadata.
    pub info: CameraInfo,
    /// Eye region used for feature extraction.
    pub roi: EyeRegion,
    /// Full-grid calibration report.
    pub report: CalibrationReport,
    /// Quick center recalibration report, when requested.
    pub quick_report: Option<CalibrationReport>,
    /// Number of live gaze predictions printed.
    pub live_samples: usize,
}

/// Errors from live camera commands.
#[derive(Debug, Error)]
pub enum LiveError {
    /// The experiment directory is invalid.
    #[error(transparent)]
    Load(#[from] LoadError),
    /// Camera capture failed.
    #[error(transparent)]
    Camera(#[from] CameraError),
    /// Calibration failed.
    #[error(transparent)]
    Model(#[from] ModelError),
    /// Terminal input or output failed.
    #[error("terminal I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// Standard input ended before calibration finished.
    #[error("unexpected end of input")]
    EndOfInput,
    /// The normalized eye region does not fit the negotiated frame.
    #[error("the eye region is invalid for the negotiated camera format")]
    InvalidEyeRegion,
    /// No frames arrived in time.
    #[error("no camera frames arrived within {0:?}")]
    NoFrames(Duration),
    /// A file could not be written.
    #[error("could not write `{path}`: {source}")]
    Write {
        /// Attempted path.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The graphical calibration window failed.
    #[error("calibration window failed: {0}")]
    Window(String),
}

/// Open the camera from an experiment directory and measure it.
pub fn probe(
    experiment_dir: &Path,
    seconds: f64,
    save_roi: Option<&Path>,
    device: Option<usize>,
) -> Result<CameraProbe, LiveError> {
    let loaded = yet_stim::load_experiment(experiment_dir)?;
    let mut camera = Camera::open(capture_config(&loaded.config.camera, device))?;
    let roi = eye_region(&loaded.config.camera, camera.info())?;

    let duration = Duration::from_secs_f64(seconds.max(0.0));
    let start = Instant::now();
    let mut consumed = 0_u64;
    let mut last_frame: Option<GrayFrame> = None;
    let mut quadrants = None;

    while start.elapsed() < duration {
        if let Some(frame) = camera.latest_frame() {
            consumed += 1;
            if let Some(features) = quad_brightness(&frame, roi) {
                quadrants = Some(features);
            }
            last_frame = Some(frame);
        }
        thread::sleep(Duration::from_millis(5));
    }

    let elapsed = start.elapsed();
    let stats = camera.stats();
    let captured_fps = if elapsed.as_secs_f64() > 0.0 {
        stats.captured as f64 / elapsed.as_secs_f64()
    } else {
        0.0
    };

    let saved_roi = if let Some(path) = save_roi {
        let frame = last_frame.as_ref().ok_or(LiveError::NoFrames(duration))?;
        let crop = frame.crop(roi).ok_or(LiveError::InvalidEyeRegion)?;
        fs::write(path, crop.data()).map_err(|source| LiveError::Write {
            path: path.to_path_buf(),
            source,
        })?;
        Some(path.to_path_buf())
    } else {
        None
    };

    let info = camera.info().clone();
    camera.stop();

    Ok(CameraProbe {
        info,
        stats,
        elapsed,
        consumed,
        captured_fps,
        quadrants,
        roi,
        saved_roi,
    })
}

/// Run a terminal-driven full calibration, optional quick recalibration and
/// live gaze printout.
///
/// This command is interactive: it asks the participant to fixate each target
/// and press Enter. It does not draw the target; the presenter does that from
/// Sprint 6 onward.
pub fn calibrate(
    experiment_dir: &Path,
    options: CalibrateOptions,
) -> Result<CalibrationOutcome, LiveError> {
    let loaded = yet_stim::load_experiment(experiment_dir)?;
    let mut camera = Camera::open(capture_config(&loaded.config.camera, options.device))?;
    let roi = eye_region(&loaded.config.camera, camera.info())?;
    let grid = CalibrationGrid::new((loaded.config.display.width, loaded.config.display.height));

    println!(
        "Camera: {} ({}) — {}x{}, {}",
        camera.info().name,
        camera.info().device,
        camera.info().width,
        camera.info().height,
        camera.info().format
    );
    println!(
        "Eye region: x={} y={} {}x{}; {} frames per target",
        roi.x,
        roi.y,
        roi.width,
        roi.height,
        options.samples.max(1)
    );
    println!(
        "Look at each target on the presenter monitor, then press Enter. \
         The target is not drawn yet; use the printed coordinates."
    );

    let mut calibration = Calibration::new();
    for target in grid.targets() {
        println!(
            "Target {}/{}: ({:.0}, {:.0})  [{:.3}, {:.3}] — press Enter",
            target.index + 1,
            grid.len(),
            target.position_px.x,
            target.position_px.y,
            target.normalized.x,
            target.normalized.y
        );
        wait_for_enter()?;
        let features = average_features(&camera, roi, options.samples.max(1))?;
        calibration.record(
            GazePoint::new(target.position_px.x, target.position_px.y),
            features,
        );
        println!(
            "  nw={:.2} ne={:.2} sw={:.2} se={:.2}",
            features.nw, features.ne, features.sw, features.se
        );
    }

    let report = calibration.fit()?;
    print_report("Calibration", report);

    let quick_report = if options.quick {
        let center = grid
            .target(grid.len() / 2)
            .ok_or(LiveError::InvalidEyeRegion)?;
        println!(
            "Quick recalibration: look at the center ({:.0}, {:.0}) — press Enter",
            center.position_px.x, center.position_px.y
        );
        wait_for_enter()?;
        let features = average_features(&camera, roi, options.samples.max(1))?;
        calibration.record(
            GazePoint::new(center.position_px.x, center.position_px.y),
            features,
        );
        let report = calibration.fit()?;
        print_report("Quick recalibration", report);
        Some(report)
    } else {
        None
    };

    let mut live_samples = 0;
    if !options.skip_live {
        println!(
            "Live gaze for {:.1} s (quit with Ctrl-C):",
            options.live_seconds.max(0.0)
        );
        let deadline = Instant::now() + Duration::from_secs_f64(options.live_seconds.max(0.0));
        while Instant::now() < deadline {
            if let Some(frame) = camera.latest_frame() {
                if let Some(features) = quad_brightness(&frame, roi) {
                    if let Ok(point) = calibration.predict(features) {
                        println!("gaze: ({:.1}, {:.1})", point.x, point.y);
                        live_samples += 1;
                    }
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    let info = camera.info().clone();
    camera.stop();

    Ok(CalibrationOutcome {
        info,
        roi,
        report,
        quick_report,
        live_samples,
    })
}

pub(crate) fn capture_config(
    config: &yet_stim::CameraConfig,
    device: Option<usize>,
) -> CaptureConfig {
    CaptureConfig {
        device: device.unwrap_or(config.usb_index),
        width: config.width,
        height: config.height,
        fps: config.fps,
        format: config.format,
    }
}

pub(crate) fn eye_region(
    config: &yet_stim::CameraConfig,
    info: &CameraInfo,
) -> Result<EyeRegion, LiveError> {
    info.eye_region(config.eye_region.unwrap_or(DEFAULT_EYE_REGION))
        .ok_or(LiveError::InvalidEyeRegion)
}

fn wait_for_enter() -> Result<(), LiveError> {
    std::io::stdout().flush()?;
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line)? == 0 {
        return Err(LiveError::EndOfInput);
    }
    Ok(())
}

fn average_features(
    camera: &Camera,
    roi: EyeRegion,
    samples: usize,
) -> Result<QuadBrightness, LiveError> {
    let timeout = Duration::from_secs(3);
    let deadline = Instant::now() + timeout;
    let mut sums = [0.0_f64; 4];
    let mut count = 0_usize;

    while count < samples && Instant::now() < deadline {
        if let Some(frame) = camera.latest_frame() {
            if let Some(features) = quad_brightness(&frame, roi) {
                for (sum, value) in sums.iter_mut().zip(features.to_array()) {
                    *sum += f64::from(value);
                }
                count += 1;
            }
        }
        thread::sleep(Duration::from_millis(5));
    }

    if count == 0 {
        return Err(LiveError::NoFrames(timeout));
    }

    let count = count as f64;
    Ok(QuadBrightness::new(
        (sums[0] / count) as f32,
        (sums[1] / count) as f32,
        (sums[2] / count) as f32,
        (sums[3] / count) as f32,
    ))
}

fn print_report(label: &str, report: CalibrationReport) {
    println!(
        "{label}: {} samples, RMSE {:.2} px",
        report.samples, report.rmse
    );
}

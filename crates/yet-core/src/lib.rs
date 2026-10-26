// SPDX-License-Identifier: GPL-3.0-only

//! UI-free experiment engine and eye-tracking core for yet-rs.
//!
//! The engine is a pure state machine: [`step`] takes an event and a session
//! timestamp, and returns effects for the application to execute. The
//! experiment definition and configuration loaders live alongside it; neither
//! depends on a window, an event loop, or a clock implementation.

use std::path::{Path, PathBuf};

use thiserror::Error;

pub mod calibration;
pub mod camera;
pub mod clock;
pub mod config;
pub mod data;
pub mod engine;
pub mod experiment;
pub mod layout;
pub mod overlay;
pub mod plan;
mod rng;
pub mod tracking;

pub use calibration::{
    CalibrationError, CalibrationGrid, CalibrationPhase, CalibrationRun, CalibrationSnapshot,
    CalibrationTarget, CalibrationTiming, QuickBackground, ACTIVE_TARGET_COLOR,
    CALIBRATION_MAX_COLLECT, CALIBRATION_MIN_COLLECT, CALIBRATION_REPORT_HOLD,
    CALIBRATION_SAMPLES_PER_TARGET, CALIBRATION_SETTLE, GRID_POSITIONS, TARGET_COLOR,
    TARGET_RADIUS_PX, TARGET_STROKE_PX,
};
pub use camera::{
    validate_eye_region, Camera, CameraError, CameraFrame, CameraInfo, CameraRig, CameraRole,
    CameraStats, CaptureConfig, FrameSet, PixelFormat, PreferredFormat,
};
pub use clock::{Clock, TestClock};
pub use config::{
    load as load_config, AppConfig, AudioConfig, CameraConfig, ConfigError, DisplayConfig,
    OverlayConfig, OverlayMode, PresenterBackend, PresenterConfig,
};
pub use data::{DataError, DataSink, GazeSample, SessionIdentity};
pub use engine::{
    step, ConsoleEvent, Effect, EngineError, EngineEvent, EngineState, FrameRecord, Key, Phase,
    TrialRecord,
};
pub use experiment::{
    Block, BlockId, Experiment, ExperimentError, Stimulus, StimulusId, StimulusKind, Trial,
};
pub use overlay::{gaze_scene, GazeMarkerStyle};
pub use plan::{PlannedBlock, PlannedTrial, SessionPlan};
pub use tracking::{
    minimum_samples, quad_brightness, rec601_luma, BodyPose, Calibration, CalibrationBuffer,
    CalibrationReport, CalibrationSample, EyeRegion, FeatureExtractor, FeatureLayout,
    FeatureVector, FrameError, GazeInput, GazeModel, GazePoint, GazePredictor, GazeTrainer,
    GrayFrame, HeadPose, LinearGazeModel, LinearParameters, LinearPredictor, ModelError,
    ModelParameters, PoseSample, PredictionError, QuadBrightness, QuadExtractor, MAX_FEATURES,
    MIN_CALIBRATION_SAMPLES, MODEL_FORMAT_VERSION, OCTA_LAYOUT, QUAD_LAYOUT,
};

/// An experiment directory loaded into configuration, definition and plan.
#[derive(Debug)]
pub struct LoadedExperiment {
    /// Root directory of the experiment.
    pub root: PathBuf,
    /// Machine configuration.
    pub config: AppConfig,
    /// Experiment definition.
    pub experiment: Experiment,
    /// Ordered session plan.
    pub plan: SessionPlan,
}

/// Errors from loading a complete experiment directory.
#[derive(Debug, Error)]
pub enum LoadError {
    /// The machine configuration is invalid.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The experiment definition is invalid.
    #[error(transparent)]
    Experiment(#[from] ExperimentError),
}

/// Load `config.toml`, `Experiment.csv` and `Stimuli.csv` from a directory.
///
/// Media paths are resolved relative to `root`.
pub fn load_experiment(root: impl AsRef<Path>) -> Result<LoadedExperiment, LoadError> {
    let root = root.as_ref().to_path_buf();
    let config = config::load(root.join("config.toml"))?;
    let experiment = experiment::load(&root)?;
    let plan = plan::build(&experiment)?;
    Ok(LoadedExperiment {
        root,
        config,
        experiment,
        plan,
    })
}

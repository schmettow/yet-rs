// SPDX-License-Identifier: GPL-3.0-only

//! Stimulus, media and presentation protocols for yet-rs.
//!
//! This crate owns the UI-free half of an experiment: the pure engine, the
//! experiment/plan/configuration model, CSV recording, pre-decoded media and
//! audio, calibration flow, and the presentation protocols. It deliberately
//! does not depend on egui, eframe or any windowing library; a presenter such
//! as [`yet-egui`](https://crates.io/crates/yet-egui) implements
//! [`Presenter`] and [`OverlayBackend`].
//!
//! Scenes are expressed in monitor-local physical pixels. The boundary to
//! logical points (egui) is the presenter's responsibility.

pub mod calibration;
pub mod config;
pub mod data;
pub mod engine;
pub mod experiment;
pub mod geometry;
pub mod layout;
pub mod media;
pub mod overlay;
pub mod plan;
pub mod playback;
pub mod protocol;
pub mod recording;
mod rng;
pub mod scene;

pub use crate::calibration::{
    CalibrationError, CalibrationGrid, CalibrationPhase, CalibrationRun, CalibrationSnapshot,
    CalibrationTarget, CalibrationTiming, QuickBackground, ACTIVE_TARGET_COLOR,
    CALIBRATION_MAX_COLLECT, CALIBRATION_MIN_COLLECT, CALIBRATION_REPORT_HOLD,
    CALIBRATION_SAMPLES_PER_TARGET, CALIBRATION_SETTLE, GRID_POSITIONS, TARGET_COLOR,
    TARGET_RADIUS_PX, TARGET_STROKE_PX,
};
pub use crate::config::{
    load as load_config, AppConfig, AudioConfig, CameraConfig, ConfigError, DisplayConfig,
    OverlayConfig, OverlayMode, PreferredFormat, PresenterBackend, PresenterConfig,
};
pub use crate::data::{DataError, DataSink, GazeSample, SessionIdentity};
pub use crate::engine::{
    step, ConsoleEvent, Effect, EngineError, EngineEvent, EngineState, FrameRecord, Key, Phase,
    TrialRecord,
};
pub use crate::experiment::{
    Block, BlockId, Experiment, ExperimentError, Stimulus, StimulusId, StimulusKind, Trial,
};
pub use crate::geometry::{PointPx, RectPx, ScreenGeometry, SizePx};
pub use crate::overlay::{gaze_scene, GazeMarkerStyle};
pub use crate::plan::{PlannedBlock, PlannedTrial, SessionPlan};
pub use crate::protocol::{
    AudioError, AudioPlayer, ClockSource, Frame, FrameSource, FrameStamp, MediaError,
    OverlayBackend, OverlayError, OverlayStats, PresentStats, Presenter, PresenterCommand,
    PresenterError,
};
pub use crate::recording::RecordingPresenter;
pub use crate::scene::{Anchor, Color, DrawCommand, Filter, FontId, MediaId, Scene};
pub use yet_core::SessionTime;

#[cfg(target_os = "linux")]
pub use crate::overlay::LayerShellOverlay;

use std::path::{Path, PathBuf};

use thiserror::Error;

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

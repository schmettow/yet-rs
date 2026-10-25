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
pub mod clock;
pub mod config;
pub mod data;
pub mod engine;
pub mod experiment;
pub mod layout;
pub mod plan;
mod rng;
pub mod tracking;

pub use calibration::{CalibrationGrid, CalibrationTarget};
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
pub use plan::{PlannedBlock, PlannedTrial, SessionPlan};
pub use tracking::{CalibrationSample, GazePoint, QuadBrightness};

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

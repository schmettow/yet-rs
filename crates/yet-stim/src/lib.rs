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
pub mod legacy;
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
pub use crate::overlay::{append_gaze_marker, gaze_scene, GazeMarkerStyle};
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

use std::fmt;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Which on-disk layout an experiment directory uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceKind {
    /// Native yet-rs layout: `config.toml` and a root `Stimuli.csv`.
    Native,
    /// `yet_zero` Python layout: `Config.csv` and `Stimuli/Stimuli.csv`.
    YetZeroPython,
}

impl WorkspaceKind {
    /// Short label for logs and plan output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::YetZeroPython => "yet_zero Python (Config.csv)",
        }
    }
}

impl fmt::Display for WorkspaceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An experiment directory loaded into configuration, definition and plan.
#[derive(Debug)]
pub struct LoadedExperiment {
    /// Root directory of the experiment.
    pub root: PathBuf,
    /// Detected workspace layout.
    pub kind: WorkspaceKind,
    /// Machine configuration.
    pub config: AppConfig,
    /// Experiment definition.
    pub experiment: Experiment,
    /// Ordered session plan.
    pub plan: SessionPlan,
    /// Default directory for result CSVs (`root` or Python's `Data/`).
    pub result_dir: PathBuf,
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
    /// A Python workspace is invalid.
    #[error(transparent)]
    Legacy(#[from] legacy::LegacyError),
    /// The directory is neither a native nor a Python workspace.
    #[error(
        "`{root}` is neither a yet-rs workspace (a root `Stimuli.csv`) nor a \
         yet_zero Python workspace (`Config.csv` and `Stimuli/Stimuli.csv`)"
    )]
    UnrecognizedWorkspace {
        /// Directory that was inspected.
        root: PathBuf,
    },
}

/// Load a native or Python `yet_zero` workspace from a directory.
///
/// Detection is by layout: a root `Stimuli.csv` selects the native loader, so
/// a missing `config.toml` is still reported as a configuration error. A
/// `Config.csv` selects the Python compatibility loader. Media paths are
/// resolved relative to `root`.
pub fn load_experiment(root: impl AsRef<Path>) -> Result<LoadedExperiment, LoadError> {
    let root = root.as_ref().to_path_buf();

    // A Python workspace may also carry an optional `config.toml` for machine
    // settings, so detect it by its own markers before the native layout.
    if root.join("Config.csv").is_file() && root.join("Stimuli").is_dir() {
        let legacy = legacy::load(&root)?;
        return Ok(LoadedExperiment {
            root,
            kind: WorkspaceKind::YetZeroPython,
            config: legacy.config,
            experiment: legacy.experiment,
            plan: legacy.plan,
            result_dir: legacy.result_dir,
        });
    }

    if root.join("Stimuli.csv").is_file()
        || root.join("config.toml").is_file()
        || root.join("Experiment.csv").is_file()
    {
        return load_native(root);
    }

    Err(LoadError::UnrecognizedWorkspace { root })
}

fn load_native(root: PathBuf) -> Result<LoadedExperiment, LoadError> {
    let config = config::load(root.join("config.toml"))?;
    let experiment = experiment::load(&root)?;
    let plan = plan::build(&experiment)?;
    Ok(LoadedExperiment {
        result_dir: root.clone(),
        root,
        kind: WorkspaceKind::Native,
        config,
        experiment,
        plan,
    })
}

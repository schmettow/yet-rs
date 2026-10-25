// SPDX-License-Identifier: GPL-3.0-only

//! egui experimenter console for yet-rs.
//!
//! The console is a view of [`yet_core`]'s engine and calibration state. It
//! sends [`ExperimentCommand`]s over a channel and never renders the stimulus;
//! that remains the presenter's job. It does display a small camera preview,
//! which is a diagnostic view and not part of the stimulus path.

mod command;
mod state;
mod ui;

pub use command::ExperimentCommand;
pub use state::{
    CalibrationStatus, ConsoleState, LogLevel, LogLine, PresenterStatus, PreviewFrame,
    PreviewState, MAX_LOG_LINES,
};
pub use ui::{ConsoleUi, MonitorChoice, MONITOR_CHOICES};

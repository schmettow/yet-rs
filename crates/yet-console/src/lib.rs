// SPDX-License-Identifier: GPL-3.0-only

//! egui experimenter console for yet-rs.
//!
//! The console is a view of [`yet_core`]'s engine state. It sends
//! [`ExperimentCommand`]s over a channel and never renders the stimulus; that
//! remains the presenter's job. The console UI can therefore be tested without
//! a stimulus, a camera, or a data sink.

mod command;
mod state;
mod ui;

pub use command::ExperimentCommand;
pub use state::{ConsoleState, LogLevel, LogLine, PresenterStatus, MAX_LOG_LINES};
pub use ui::{ConsoleUi, MONITOR_CHOICES};

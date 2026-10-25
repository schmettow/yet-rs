// SPDX-License-Identifier: GPL-3.0-only

//! Commands the console sends to the experiment controller.

/// A session-level request from the experimenter.
///
/// The console never manipulates engine state directly. It sends one of these
/// over a channel; the controller maps it to an engine event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExperimentCommand {
    /// Start the planned session.
    Start,
    /// Pause the clocks and stop advancing trials.
    Pause,
    /// Resume a paused session.
    Resume,
    /// Stop the session and keep the partial result file.
    Abort,
    /// Abort if necessary, flush the result file, and stop the controller.
    Shutdown,
}

impl ExperimentCommand {
    /// Short label for logs and tests.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Abort => "abort",
            Self::Shutdown => "shutdown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ExperimentCommand;

    #[test]
    fn commands_have_stable_labels() {
        assert_eq!(ExperimentCommand::Start.label(), "start");
        assert_eq!(ExperimentCommand::Pause.label(), "pause");
        assert_eq!(ExperimentCommand::Resume.label(), "resume");
        assert_eq!(ExperimentCommand::Abort.label(), "abort");
        assert_eq!(ExperimentCommand::Shutdown.label(), "shutdown");
    }
}

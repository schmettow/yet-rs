// SPDX-License-Identifier: GPL-3.0-only

//! Console state shared between the experiment controller and the UI.
//!
//! The console is a view of the engine, never the engine itself. It holds a
//! bounded log, progress counters, and the presenter-monitor selection. All
//! fields are plain data so the console can be driven headlessly in tests.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use yet_core::{
    BlockId, CalibrationPhase, CalibrationReport, CalibrationSnapshot, GazePoint, Phase, StimulusId,
};

/// Maximum number of log lines kept in memory.
pub const MAX_LOG_LINES: usize = 300;

/// Severity of a console log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    /// Normal progress.
    Info,
    /// Recoverable or suspicious event.
    Warning,
    /// Error that stopped the session.
    Error,
}

/// One timestamped log line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    /// Experiment time at which the line was recorded.
    pub at: Duration,
    /// Severity.
    pub level: LogLevel,
    /// Message text.
    pub message: String,
}

/// A snapshot of presenter progress, supplied by the presenter crate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PresenterStatus {
    /// Whether the stimulus viewport has settled at its final monitor size.
    pub ready: bool,
    /// Configured monitor index.
    pub monitor: usize,
    /// Whether the stimulus viewport is borderless fullscreen.
    pub fullscreen: bool,
    /// Actual viewport size in physical pixels.
    pub size_px: (u32, u32),
    /// Actual physical pixels per logical point.
    pub scale_factor: f32,
    /// Scenes painted by the viewport.
    pub paints: u64,
    /// Scenes handed to the presenter.
    pub presents: u64,
    /// Scenes replaced before they were painted.
    pub coalesced: u64,
    /// Measured paint rate while scenes arrive.
    pub paint_hz: Option<f32>,
    /// Interval between the two most recent presents.
    pub last_dt: Option<Duration>,
    /// Loaded media entries.
    pub media_count: usize,
    /// Whether the stimulus viewport has input focus.
    pub focused: bool,
}

/// State of the calibration flow and its last reports.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalibrationStatus {
    /// Whether a calibration run owns the presenter scene.
    pub active: bool,
    /// Current calibration phase.
    pub phase: CalibrationPhase,
    /// Active target index, when one is active.
    pub target: Option<usize>,
    /// Number of targets in the grid.
    pub total: usize,
    /// Targets recorded in this run.
    pub completed: usize,
    /// Frames collected for the active target.
    pub collected: usize,
    /// Last successful full report.
    pub report: Option<CalibrationReport>,
    /// Last successful quick report.
    pub quick_report: Option<CalibrationReport>,
    /// Quick RMSE minus the baseline (previous quick or full) RMSE.
    pub drift_rmse: Option<f32>,
    /// Whether a fitted model is active.
    pub fitted: bool,
    /// Calibration or camera error, shown in the calibration panel.
    pub error: Option<String>,
}

impl CalibrationStatus {
    /// Fraction of targets completed, in `0.0..=1.0`.
    pub fn progress(&self) -> f32 {
        if matches!(
            self.phase,
            CalibrationPhase::QuickSettle
                | CalibrationPhase::QuickCollect
                | CalibrationPhase::QuickReport
        ) {
            self.completed.min(1) as f32
        } else if self.total == 0 {
            0.0
        } else {
            (self.completed as f32 / self.total as f32).clamp(0.0, 1.0)
        }
    }

    /// Whether the active target has progressed into collection.
    pub fn collecting(&self) -> bool {
        matches!(
            self.phase,
            CalibrationPhase::Collect | CalibrationPhase::QuickCollect
        )
    }
}

/// Largest acceptable audio/video drift before the console logs a warning.
pub const MAX_AUDIO_DRIFT_MS: f32 = 50.0;

/// Minimum interval between audio warnings.
pub const AUDIO_WARNING_INTERVAL: Duration = Duration::from_secs(2);

/// One grayscale camera preview sent from the controller to the console.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewFrame {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Row-major 8-bit grayscale pixels.
    pub data: Arc<[u8]>,
    /// Monotonic generation; the UI re-uploads the texture when it changes.
    pub generation: u64,
    /// Predicted gaze in monitor pixels.
    pub gaze: Option<GazePoint>,
    /// Predicted gaze normalized to the presenter size.
    pub gaze_normalized: Option<GazePoint>,
}

/// Live camera preview state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PreviewState {
    /// Whether the experimenter enabled the preview.
    pub enabled: bool,
    /// Ask the controller to retry opening the camera.
    pub retry: bool,
    /// Camera or ROI error.
    pub error: Option<String>,
    /// Latest downscaled eye-ROI frame.
    pub frame: Option<PreviewFrame>,
    /// Capture rate reported by the camera thread.
    pub capture_fps: Option<f32>,
}

/// Everything the console displays.
#[derive(Clone, Debug)]
pub struct ConsoleState {
    /// Experiment identifier.
    pub experiment_id: String,
    /// Participant identifier written to the result CSV.
    pub participant: String,
    /// Engine phase.
    pub phase: Phase,
    /// Presenter monitor selected before the session starts.
    pub presenter_monitor: usize,
    /// Console monitor from the configuration; recorded, not enforced.
    pub console_monitor: usize,
    /// Whether the presenter is borderless fullscreen.
    pub fullscreen: bool,
    /// Number of planned trials.
    pub trials_total: usize,
    /// Number of completed trials.
    pub trials_completed: usize,
    /// Number of trials that finished late.
    pub late_trials: usize,
    /// Largest lateness of any trial.
    pub max_late: Duration,
    /// Number of gaze samples written.
    pub samples: usize,
    /// Media frames dropped or coalesced since the session started.
    pub dropped_frames: u64,
    /// Latest audio/video clock drift in milliseconds.
    pub audio_drift_ms: Option<f32>,
    /// Latest media presentation timestamp.
    pub media_pts: Option<Duration>,
    /// Largest absolute drift seen in this session.
    pub max_audio_drift_ms: Option<f32>,
    /// Cumulative audio underrun transitions.
    pub audio_underruns: u32,
    /// When the last drift warning was logged, for rate limiting.
    pub last_audio_warning: Option<Duration>,
    /// Active block.
    pub current_block: Option<BlockId>,
    /// Zero-based index of the active trial within its block.
    pub current_trial: Option<usize>,
    /// Active stimulus.
    pub current_stimulus: Option<StimulusId>,
    /// Experiment time excluding pauses.
    pub elapsed: Duration,
    /// Console wall time since launch, used for log timestamps.
    pub log_elapsed: Duration,
    /// Fatal error, shown prominently.
    pub error: Option<String>,
    /// Calibration flow and reports.
    pub calibration: CalibrationStatus,
    /// Live camera preview.
    pub preview: PreviewState,
    /// Seconds from console launch to the first presented trial, when reached.
    pub first_trial_seconds: Option<f32>,
    /// Bounded event log.
    pub log: VecDeque<LogLine>,
    /// Set when the controller stopped and the console may close.
    pub finished: bool,
}

impl ConsoleState {
    /// Create a console state for a loaded experiment.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        experiment_id: impl Into<String>,
        participant: impl Into<String>,
        presenter_monitor: usize,
        console_monitor: usize,
        fullscreen: bool,
        trials_total: usize,
    ) -> Self {
        Self {
            experiment_id: experiment_id.into(),
            participant: participant.into(),
            phase: Phase::Idle,
            presenter_monitor,
            console_monitor,
            fullscreen,
            trials_total,
            trials_completed: 0,
            late_trials: 0,
            max_late: Duration::ZERO,
            samples: 0,
            dropped_frames: 0,
            audio_drift_ms: None,
            media_pts: None,
            max_audio_drift_ms: None,
            audio_underruns: 0,
            last_audio_warning: None,
            current_block: None,
            current_trial: None,
            current_stimulus: None,
            elapsed: Duration::ZERO,
            log_elapsed: Duration::ZERO,
            error: None,
            calibration: CalibrationStatus::default(),
            preview: PreviewState::default(),
            first_trial_seconds: None,
            log: VecDeque::new(),
            finished: false,
        }
    }

    /// Update the calibration status from a run snapshot.
    ///
    /// Detects a newly delivered quick report to compute the drift against the
    /// previous quick report, or the full report when this is the first quick
    /// recalibration.
    pub fn apply_calibration(&mut self, snapshot: CalibrationSnapshot) {
        let status = &mut self.calibration;
        if snapshot.quick_report != status.quick_report {
            let baseline = status.quick_report.or(status.report);
            status.drift_rmse = match (snapshot.quick_report, baseline) {
                (Some(quick), Some(baseline)) => Some(quick.rmse - baseline.rmse),
                _ => None,
            };
        }
        if snapshot.phase.is_active() || snapshot.phase == CalibrationPhase::Done {
            status.error = None;
        }

        status.active = snapshot.phase.is_active();
        status.phase = snapshot.phase;
        status.target = snapshot.target;
        status.total = snapshot.total;
        status.completed = snapshot.completed;
        status.collected = snapshot.collected;
        status.report = snapshot.report;
        status.quick_report = snapshot.quick_report;
        status.fitted = snapshot.fitted;
    }

    /// Record presentation statistics from one frame.
    ///
    /// Returns a warning message when audio drift or an underrun should be
    /// logged, rate-limited to one message every two seconds.
    pub fn record_present(
        &mut self,
        dropped: u32,
        drift_ms: Option<f32>,
        underruns: u32,
        media_pts: Option<Duration>,
    ) -> Option<String> {
        self.dropped_frames = self.dropped_frames.saturating_add(u64::from(dropped));
        self.audio_underruns = self.audio_underruns.saturating_add(underruns);
        if media_pts.is_some() {
            self.media_pts = media_pts;
        }
        if let Some(drift) = drift_ms.filter(|drift| drift.is_finite()) {
            self.audio_drift_ms = Some(drift);
            let magnitude = drift.abs();
            self.max_audio_drift_ms = Some(
                self.max_audio_drift_ms
                    .map_or(magnitude, |max| max.max(magnitude)),
            );
        }

        let due = self
            .last_audio_warning
            .is_none_or(|last| self.log_elapsed.saturating_sub(last) >= AUDIO_WARNING_INTERVAL);
        if !due {
            return None;
        }
        let message = if underruns > 0 {
            Some("audio underrun".to_owned())
        } else {
            self.audio_drift_ms
                .filter(|drift| drift.abs() > MAX_AUDIO_DRIFT_MS)
                .map(|drift| format!("audio drift {drift:+.0} ms"))
        };
        if message.is_some() {
            self.last_audio_warning = Some(self.log_elapsed);
        }
        message
    }

    /// Append a log line, dropping the oldest line beyond [`MAX_LOG_LINES`].
    pub fn log(&mut self, level: LogLevel, message: impl Into<String>) {
        if self.log.len() == MAX_LOG_LINES {
            self.log.pop_front();
        }
        self.log.push_back(LogLine {
            at: self.log_elapsed,
            level,
            message: message.into(),
        });
    }

    /// Human-readable phase label.
    pub fn phase_label(&self) -> &'static str {
        match self.phase {
            Phase::Idle => "idle",
            Phase::Running => "running",
            Phase::Paused => "paused",
            Phase::Finished => "finished",
            Phase::Aborted => "aborted",
        }
    }

    /// Fraction of planned trials completed, in `0.0..=1.0`.
    pub fn progress(&self) -> f32 {
        if self.trials_total == 0 {
            0.0
        } else {
            (self.trials_completed as f32 / self.trials_total as f32).clamp(0.0, 1.0)
        }
    }

    /// Whether a session can start.
    pub fn can_start(&self) -> bool {
        self.phase == Phase::Idle && self.error.is_none() && !self.calibration.active
    }

    /// Clamp the presenter monitor to the monitors the platform reports.
    ///
    /// An out-of-range `display.presenter_monitor` would otherwise put the
    /// presenter in a plain 800×600 window. Falls back to the last monitor (the
    /// second one on a two-monitor desk) and returns a message for the log.
    pub fn adopt_monitors(&mut self, available: usize) -> Option<String> {
        if available == 0 || self.presenter_monitor < available {
            return None;
        }
        let configured = self.presenter_monitor;
        self.presenter_monitor = available - 1;
        Some(format!(
            "display.presenter_monitor {configured} is out of range for {available} monitors; \
             using monitor {} (index {})",
            available, self.presenter_monitor
        ))
    }

    /// Whether the session can pause.
    pub fn can_pause(&self) -> bool {
        self.phase == Phase::Running && !self.calibration.active
    }

    /// Whether the session can resume.
    pub fn can_resume(&self) -> bool {
        self.phase == Phase::Paused && !self.calibration.active
    }

    /// Whether the session can be aborted.
    pub fn can_abort(&self) -> bool {
        matches!(self.phase, Phase::Running | Phase::Paused) && !self.calibration.active
    }

    /// Whether a full calibration can start.
    pub fn can_calibrate(&self, presenter_ready: bool) -> bool {
        presenter_ready
            && !self.calibration.active
            && matches!(self.phase, Phase::Idle | Phase::Finished)
    }

    /// Whether a quick recalibration can start.
    pub fn can_quick_calibrate(&self, presenter_ready: bool) -> bool {
        presenter_ready
            && !self.calibration.active
            && self.calibration.fitted
            && matches!(self.phase, Phase::Idle | Phase::Paused | Phase::Finished)
    }

    /// Whether the active calibration can be cancelled.
    pub fn can_cancel_calibration(&self) -> bool {
        self.calibration.active
    }

    /// Whether the experimenter may change presenter settings.
    pub fn can_configure(&self) -> bool {
        !self.calibration.active
            && matches!(self.phase, Phase::Idle | Phase::Finished | Phase::Aborted)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use yet_core::{CalibrationPhase, CalibrationReport, CalibrationSnapshot, Phase};

    use super::{ConsoleState, LogLevel, MAX_LOG_LINES};

    fn state() -> ConsoleState {
        ConsoleState::new("demo", "42", 1, 0, true, 4)
    }

    #[test]
    fn an_out_of_range_presenter_monitor_falls_back_to_the_last_monitor() {
        let mut state = state();
        state.presenter_monitor = 2;

        let message = state.adopt_monitors(2).expect("a warning");

        assert_eq!(state.presenter_monitor, 1);
        assert!(message.contains("out of range"), "{message}");
        assert!(message.contains("monitor 2"), "{message}");
        assert_eq!(state.adopt_monitors(2), None);
    }

    #[test]
    fn a_valid_presenter_monitor_is_left_alone() {
        let mut state = state();
        state.presenter_monitor = 1;

        assert_eq!(state.adopt_monitors(2), None);
        assert_eq!(state.presenter_monitor, 1);
        assert_eq!(state.adopt_monitors(0), None);
    }

    #[test]
    fn presentation_stats_accumulate_and_warn_with_backpressure() {
        let mut state = state();
        state.log_elapsed = Duration::from_secs(10);

        let warning = state.record_present(2, Some(80.0), 0, Some(Duration::from_millis(40)));
        assert!(warning.unwrap().contains("drift"));
        assert_eq!(state.dropped_frames, 2);
        assert_eq!(state.max_audio_drift_ms, Some(80.0));

        state.log_elapsed = Duration::from_secs(11);
        assert_eq!(state.record_present(1, Some(90.0), 1, None), None);
        assert_eq!(state.dropped_frames, 3);
        assert_eq!(state.max_audio_drift_ms, Some(90.0));
        assert_eq!(state.audio_underruns, 1);

        state.log_elapsed = Duration::from_secs(12);
        let warning = state.record_present(0, None, 1, None).unwrap();
        assert!(warning.contains("underrun"));
        assert_eq!(state.audio_underruns, 2);
    }

    #[test]
    fn small_drift_is_displayed_without_a_warning() {
        let mut state = state();

        assert_eq!(state.record_present(0, Some(3.0), 0, None), None);
        assert_eq!(state.audio_drift_ms, Some(3.0));
        assert_eq!(state.max_audio_drift_ms, Some(3.0));
    }

    #[test]
    fn the_log_is_bounded_and_keeps_the_newest_lines() {
        let mut state = state();

        for index in 0..(MAX_LOG_LINES + 5) {
            state.log(LogLevel::Info, format!("line {index}"));
        }

        assert_eq!(state.log.len(), MAX_LOG_LINES);
        assert_eq!(state.log.front().unwrap().message, "line 5");
        assert_eq!(
            state.log.back().unwrap().message,
            format!("line {}", MAX_LOG_LINES + 4)
        );
    }

    #[test]
    fn progress_is_bounded_and_handles_an_empty_plan() {
        let mut state = state();
        assert_eq!(state.progress(), 0.0);

        state.trials_completed = 2;
        assert_eq!(state.progress(), 0.5);

        state.trials_completed = 99;
        assert_eq!(state.progress(), 1.0);

        state.trials_total = 0;
        assert_eq!(state.progress(), 0.0);
    }

    #[test]
    fn controls_follow_the_engine_phase() {
        let mut state = state();
        assert!(state.can_start());
        assert!(!state.can_pause());
        assert!(!state.can_resume());
        assert!(!state.can_abort());
        assert!(state.can_configure());

        state.phase = Phase::Running;
        assert!(!state.can_start());
        assert!(state.can_pause());
        assert!(state.can_abort());
        assert!(!state.can_configure());

        state.phase = Phase::Paused;
        assert!(state.can_resume());
        assert!(state.can_abort());

        state.phase = Phase::Finished;
        assert!(!state.can_start());
        assert!(state.can_configure());
    }

    #[test]
    fn an_error_blocks_start() {
        let mut state = state();
        state.error = Some("missing stimulus".to_owned());

        assert!(!state.can_start());
    }

    #[test]
    fn phase_labels_are_stable_for_the_ui() {
        let mut state = state();
        for (phase, label) in [
            (Phase::Idle, "idle"),
            (Phase::Running, "running"),
            (Phase::Paused, "paused"),
            (Phase::Finished, "finished"),
            (Phase::Aborted, "aborted"),
        ] {
            state.phase = phase;
            assert_eq!(state.phase_label(), label);
        }
    }

    fn snapshot(phase: CalibrationPhase, fitted: bool) -> CalibrationSnapshot {
        CalibrationSnapshot {
            phase,
            target: Some(0),
            total: 9,
            completed: 1,
            collected: 3,
            report: None,
            quick_report: None,
            fitted,
        }
    }

    #[test]
    fn calibration_controls_follow_the_run_and_the_presenter() {
        let mut state = state();
        assert!(state.can_calibrate(true));
        assert!(!state.can_calibrate(false));
        assert!(!state.can_quick_calibrate(true));

        state.calibration.fitted = true;
        assert!(state.can_quick_calibrate(true));

        state.phase = Phase::Running;
        assert!(!state.can_calibrate(true));
        assert!(!state.can_quick_calibrate(true));

        state.calibration.active = true;
        assert!(!state.can_start());
        assert!(!state.can_pause());
        assert!(!state.can_abort());
        assert!(state.can_cancel_calibration());
        assert!(!state.can_configure());
    }

    #[test]
    fn a_calibration_snapshot_updates_progress_and_fit() {
        let mut state = state();
        state.apply_calibration(snapshot(CalibrationPhase::Collect, true));

        assert!(state.calibration.active);
        assert_eq!(state.calibration.phase, CalibrationPhase::Collect);
        assert_eq!(state.calibration.completed, 1);
        assert_eq!(state.calibration.collected, 3);
        assert_eq!(state.calibration.progress(), 1.0 / 9.0);
        assert!(state.calibration.collecting());
    }

    #[test]
    fn quick_reports_show_drift_against_the_previous_baseline() {
        let mut state = state();
        let mut full = snapshot(CalibrationPhase::FullReport, true);
        full.report = Some(CalibrationReport {
            samples: 18,
            rmse: 10.0,
        });
        state.apply_calibration(full);

        let mut quick = snapshot(CalibrationPhase::QuickReport, true);
        quick.report = full.report;
        quick.quick_report = Some(CalibrationReport {
            samples: 19,
            rmse: 11.5,
        });
        state.apply_calibration(quick);
        assert_eq!(state.calibration.drift_rmse, Some(1.5));

        let mut next = quick;
        next.quick_report = Some(CalibrationReport {
            samples: 20,
            rmse: 11.0,
        });
        state.apply_calibration(next);
        assert_eq!(state.calibration.drift_rmse, Some(-0.5));
    }

    #[test]
    fn the_camera_preview_is_off_by_default() {
        let state = state();
        assert!(!state.preview.enabled);
        assert!(state.preview.frame.is_none());
        assert!(state.preview.error.is_none());
    }
}

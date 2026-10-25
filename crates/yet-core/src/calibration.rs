// SPDX-License-Identifier: GPL-3.0-only

//! Calibration targets and the calibration flow.
//!
//! [`CalibrationGrid`] is pure geometry: it produces [`Scene`]s for a presenter
//! and normalized targets for [`CalibrationSample`]s. [`CalibrationRun`] is the
//! UI-free state machine behind the console's calibration panel. It never reads
//! a camera; the application feeds it [`QuadBrightness`] values and presents the
//! scene it returns.
//!
//! [`CalibrationSample`]: crate::tracking::CalibrationSample

use std::time::Duration;

use thiserror::Error;
use yet_present::{Color, MediaId, PointPx, RectPx, Scene, SessionTime};

use crate::layout::{
    BACKGROUND_COLOR, INSTRUCTION_FONT, INSTRUCTION_FONT_SIZE_DIVISOR,
    INSTRUCTION_RELATIVE_POSITION,
};
use crate::tracking::{Calibration, CalibrationReport, GazePoint, ModelError, QuadBrightness};

/// Relative positions of the 3x3 grid, taken from the Python prototype.
pub const GRID_POSITIONS: [f32; 3] = [0.125, 0.5, 0.875];

/// Radius of a calibration target in physical pixels.
pub const TARGET_RADIUS_PX: f32 = 20.0;

/// Stroke width of a calibration target in physical pixels.
pub const TARGET_STROKE_PX: f32 = 10.0;

/// Color of an inactive calibration target.
pub const TARGET_COLOR: Color = Color::rgb(160, 160, 160);

/// Color of the active calibration target.
pub const ACTIVE_TARGET_COLOR: Color = Color::rgb(255, 120, 0);

/// Time a newly active target is shown before frames are collected.
pub const CALIBRATION_SETTLE: Duration = Duration::from_millis(800);

/// Minimum collection time per target.
pub const CALIBRATION_MIN_COLLECT: Duration = Duration::from_millis(500);

/// Maximum collection time per target.
pub const CALIBRATION_MAX_COLLECT: Duration = Duration::from_millis(1500);

/// Time a calibration report stays on the presenter.
pub const CALIBRATION_REPORT_HOLD: Duration = Duration::from_secs(2);

/// Frames averaged per calibration target.
pub const CALIBRATION_SAMPLES_PER_TARGET: usize = 20;

/// One target of the calibration grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalibrationTarget {
    /// Zero-based row-major index.
    pub index: usize,
    /// Position as a fraction of the monitor size.
    pub normalized: GazePoint,
    /// Position in monitor-local physical pixels.
    pub position_px: PointPx,
}

/// A 3x3 calibration grid for one monitor size.
///
/// Targets are ordered row-major (top-left to bottom-right). The Python
/// prototype iterates `itertools.product(x, y)`, which visits the same nine
/// points in column-major order; the order is presentation metadata, not part
/// of the measurement.
#[derive(Clone, Debug, PartialEq)]
pub struct CalibrationGrid {
    surface_px: (u32, u32),
    targets: Vec<CalibrationTarget>,
}

impl CalibrationGrid {
    /// Build the grid for a surface in physical pixels.
    pub fn new(surface_px: (u32, u32)) -> Self {
        let (width, height) = surface_px;
        let mut targets = Vec::with_capacity(GRID_POSITIONS.len() * GRID_POSITIONS.len());

        for (row, y) in GRID_POSITIONS.into_iter().enumerate() {
            for (column, x) in GRID_POSITIONS.into_iter().enumerate() {
                targets.push(CalibrationTarget {
                    index: row * GRID_POSITIONS.len() + column,
                    normalized: GazePoint::new(x, y),
                    position_px: PointPx::new(
                        (x * width as f32).round(),
                        (y * height as f32).round(),
                    ),
                });
            }
        }

        Self {
            surface_px,
            targets,
        }
    }

    /// Monitor size in physical pixels.
    pub const fn surface_px(&self) -> (u32, u32) {
        self.surface_px
    }

    /// Targets in row-major order.
    pub fn targets(&self) -> &[CalibrationTarget] {
        &self.targets
    }

    /// Number of targets.
    pub fn len(&self) -> usize {
        self.targets.len()
    }

    /// Whether the grid has no targets. A grid is never empty in practice.
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// The target at `index`, if it exists.
    pub fn target(&self, index: usize) -> Option<CalibrationTarget> {
        self.targets.get(index).copied()
    }

    /// A scene with every target; the active one is orange.
    ///
    /// An out-of-range index draws no active target.
    pub fn scene(&self, active: Option<usize>) -> Scene {
        let mut scene = Scene::with_capacity(self.targets.len() + 2);
        scene.clear(BACKGROUND_COLOR);

        for target in &self.targets {
            let color = if active == Some(target.index) {
                ACTIVE_TARGET_COLOR
            } else {
                TARGET_COLOR
            };
            scene.circle(
                target.position_px,
                TARGET_RADIUS_PX,
                TARGET_STROKE_PX,
                color,
            );
        }

        scene
    }

    /// The grid scene with an instruction or progress line.
    pub fn scene_with_text(&self, active: Option<usize>, text: impl Into<String>) -> Scene {
        let mut scene = self.scene(active);
        self.push_text(&mut scene, text);
        scene
    }

    /// A quick-recalibration scene: optional blurred stimulus, one target, text.
    pub fn quick_scene(
        &self,
        active: Option<usize>,
        background: Option<QuickBackground>,
        text: impl Into<String>,
    ) -> Scene {
        let mut scene = Scene::with_capacity(4);
        scene.clear(BACKGROUND_COLOR);
        if let Some(background) = background {
            scene.blurred_image(
                background.media,
                background.dest_px,
                background.radius_px,
                1.0,
            );
        }
        if let Some(target) = active.and_then(|index| self.target(index)) {
            scene.circle(
                target.position_px,
                TARGET_RADIUS_PX,
                TARGET_STROKE_PX,
                ACTIVE_TARGET_COLOR,
            );
        }
        self.push_text(&mut scene, text);
        scene
    }

    fn push_text(&self, scene: &mut Scene, text: impl Into<String>) {
        let (width, height) = self.surface_px;
        let size_px = (width.min(height) as f32 / INSTRUCTION_FONT_SIZE_DIVISOR).max(1.0);
        let pos_px = PointPx::new(
            INSTRUCTION_RELATIVE_POSITION.0 * width as f32,
            INSTRUCTION_RELATIVE_POSITION.1 * height as f32,
        );
        scene.text(
            pos_px,
            text,
            INSTRUCTION_FONT,
            size_px,
            Color::BLACK,
            yet_present::Anchor::TopLeft,
        );
    }
}

/// A blurred stimulus behind the quick-recalibration target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuickBackground {
    /// Media to blur and draw.
    pub media: MediaId,
    /// Destination rectangle in physical pixels.
    pub dest_px: RectPx,
    /// Box-blur radius in source-image pixels.
    pub radius_px: f32,
}

/// Timing of the calibration flow. The defaults match the Sprint 5 window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalibrationTiming {
    /// Time before collecting frames for a new target.
    pub settle: Duration,
    /// Minimum collection time per target.
    pub min_collect: Duration,
    /// Maximum collection time per target.
    pub max_collect: Duration,
    /// Time a report stays on the presenter.
    pub report_hold: Duration,
    /// Frames averaged per target.
    pub samples_per_target: usize,
}

impl Default for CalibrationTiming {
    fn default() -> Self {
        Self {
            settle: CALIBRATION_SETTLE,
            min_collect: CALIBRATION_MIN_COLLECT,
            max_collect: CALIBRATION_MAX_COLLECT,
            report_hold: CALIBRATION_REPORT_HOLD,
            samples_per_target: CALIBRATION_SAMPLES_PER_TARGET,
        }
    }
}

/// Phase of a [`CalibrationRun`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CalibrationPhase {
    /// No calibration has started.
    #[default]
    Idle,
    /// The active target is shown; frames are not collected yet.
    Settle,
    /// Frames are collected for the active target.
    Collect,
    /// The full-grid report is shown for [`CalibrationTiming::report_hold`].
    FullReport,
    /// The center target is shown for quick recalibration.
    QuickSettle,
    /// Frames are collected for the quick center target.
    QuickCollect,
    /// The quick report is shown.
    QuickReport,
    /// The run completed; the fitted model is active.
    Done,
    /// The run failed; a previous model, if any, is still active.
    Failed,
    /// The experimenter cancelled the run.
    Cancelled,
}

impl CalibrationPhase {
    /// Whether the calibration scene is still owned by the run.
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            Self::Settle
                | Self::Collect
                | Self::FullReport
                | Self::QuickSettle
                | Self::QuickCollect
                | Self::QuickReport
        )
    }

    /// Short label for the console.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Settle => "settle",
            Self::Collect => "collect",
            Self::FullReport => "report",
            Self::QuickSettle => "quick settle",
            Self::QuickCollect => "quick collect",
            Self::QuickReport => "quick report",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// A read-only snapshot of a calibration run for the console.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalibrationSnapshot {
    /// Current phase.
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
    /// Whether a fitted model is active.
    pub fitted: bool,
}

/// Errors from a calibration run.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum CalibrationError {
    /// No camera frames arrived for a target.
    #[error("no camera frames arrived for target {target} of {total}")]
    NoFrames {
        /// One-based target number.
        target: usize,
        /// Number of targets in the grid.
        total: usize,
    },
    /// A target index does not exist.
    #[error("calibration target {0} is missing")]
    MissingTarget(usize),
    /// The model could not be fitted.
    #[error(transparent)]
    Model(#[from] ModelError),
    /// A calibration is already running.
    #[error("a calibration is already running")]
    AlreadyActive,
    /// Quick recalibration needs an existing fit.
    #[error("quick recalibration needs an existing calibration; run a full calibration first")]
    NotFitted,
}

/// The UI-free calibration state machine.
///
/// The application calls [`Self::start`] or [`Self::quick`], then feeds new
/// quadrant features to [`Self::tick`] and presents [`Self::scene`]. A failed
/// run restores the previous model and buffer, so a bad calibration never
/// destroys a working one.
pub struct CalibrationRun {
    grid: CalibrationGrid,
    calibration: Calibration,
    backup: Option<Calibration>,
    timing: CalibrationTiming,
    phase: CalibrationPhase,
    target: Option<usize>,
    completed: usize,
    since: SessionTime,
    sums: [f64; 4],
    count: usize,
    full_report: Option<CalibrationReport>,
    quick_report: Option<CalibrationReport>,
    error: Option<String>,
    quick_background: Option<QuickBackground>,
}

impl CalibrationRun {
    /// Create a run for a grid with the default timing.
    pub fn new(grid: CalibrationGrid) -> Self {
        Self::with_timing(grid, CalibrationTiming::default())
    }

    /// Create a run with explicit timing, mostly for tests.
    pub fn with_timing(grid: CalibrationGrid, timing: CalibrationTiming) -> Self {
        Self {
            grid,
            calibration: Calibration::new(),
            backup: None,
            timing,
            phase: CalibrationPhase::Idle,
            target: None,
            completed: 0,
            since: SessionTime::ZERO,
            sums: [0.0; 4],
            count: 0,
            full_report: None,
            quick_report: None,
            error: None,
            quick_background: None,
        }
    }

    /// Start a fresh full-grid calibration.
    pub fn start(&mut self, now: SessionTime) -> Result<(), CalibrationError> {
        if self.phase.is_active() {
            return Err(CalibrationError::AlreadyActive);
        }
        self.backup = Some(self.calibration.clone());
        self.calibration = Calibration::new();
        self.phase = CalibrationPhase::Settle;
        self.target = Some(0);
        self.completed = 0;
        self.since = now;
        self.sums = [0.0; 4];
        self.count = 0;
        self.error = None;
        self.quick_background = None;
        Ok(())
    }

    /// Start a quick recalibration on the center target.
    ///
    /// The existing samples are kept and the center sample is appended, as in
    /// the Python prototype.
    pub fn quick(&mut self, now: SessionTime) -> Result<(), CalibrationError> {
        if self.phase.is_active() {
            return Err(CalibrationError::AlreadyActive);
        }
        if !self.calibration.is_fitted() {
            return Err(CalibrationError::NotFitted);
        }
        self.phase = CalibrationPhase::QuickSettle;
        self.target = Some(self.grid.len() / 2);
        self.completed = 0;
        self.since = now;
        self.sums = [0.0; 4];
        self.count = 0;
        self.error = None;
        Ok(())
    }

    /// Set the blurred stimulus behind the quick-recalibration target.
    pub fn set_quick_background(&mut self, background: Option<QuickBackground>) {
        self.quick_background = background;
    }

    /// Cancel the active run and restore any previous calibration.
    pub fn cancel(&mut self) {
        if !self.phase.is_active() {
            return;
        }
        if let Some(backup) = self.backup.take() {
            self.calibration = backup;
        }
        self.phase = CalibrationPhase::Cancelled;
        self.target = None;
        self.sums = [0.0; 4];
        self.count = 0;
        self.quick_background = None;
    }

    /// Advance the run with the features of a new camera frame.
    ///
    /// `features` must be `None` when no new frame arrived; the same frame must
    /// not be counted twice. A failed target or fit moves the run to
    /// [`CalibrationPhase::Failed`] and returns the error.
    pub fn tick(
        &mut self,
        now: SessionTime,
        features: Option<QuadBrightness>,
    ) -> Result<(), CalibrationError> {
        match self.phase {
            CalibrationPhase::Settle => {
                if now.duration_since(self.since) >= self.timing.settle {
                    self.phase = CalibrationPhase::Collect;
                    self.since = now;
                    self.sums = [0.0; 4];
                    self.count = 0;
                }
            }
            CalibrationPhase::Collect => {
                self.collect(features);
                let elapsed = now.duration_since(self.since);
                if elapsed >= self.timing.min_collect
                    && (self.count >= self.timing.samples_per_target
                        || elapsed >= self.timing.max_collect)
                {
                    self.finish_target(now)?;
                }
            }
            CalibrationPhase::FullReport => {
                if now.duration_since(self.since) >= self.timing.report_hold {
                    self.phase = CalibrationPhase::Done;
                }
            }
            CalibrationPhase::QuickSettle => {
                if now.duration_since(self.since) >= self.timing.settle {
                    self.phase = CalibrationPhase::QuickCollect;
                    self.since = now;
                    self.sums = [0.0; 4];
                    self.count = 0;
                }
            }
            CalibrationPhase::QuickCollect => {
                self.collect(features);
                let elapsed = now.duration_since(self.since);
                if elapsed >= self.timing.min_collect
                    && (self.count >= self.timing.samples_per_target
                        || elapsed >= self.timing.max_collect)
                {
                    self.finish_quick(now)?;
                }
            }
            CalibrationPhase::QuickReport => {
                if now.duration_since(self.since) >= self.timing.report_hold {
                    self.phase = CalibrationPhase::Done;
                }
            }
            CalibrationPhase::Idle
            | CalibrationPhase::Done
            | CalibrationPhase::Failed
            | CalibrationPhase::Cancelled => {}
        }
        Ok(())
    }

    /// The calibration grid.
    pub fn grid(&self) -> &CalibrationGrid {
        &self.grid
    }

    /// The current phase.
    pub const fn phase(&self) -> CalibrationPhase {
        self.phase
    }

    /// Whether the calibration scene should still be presented.
    pub const fn is_active(&self) -> bool {
        self.phase.is_active()
    }

    /// Whether a fitted model is active.
    pub fn is_fitted(&self) -> bool {
        self.calibration.is_fitted()
    }

    /// Predict a gaze position in monitor pixels.
    pub fn predict(&self, features: QuadBrightness) -> Result<GazePoint, ModelError> {
        self.calibration.predict(features)
    }

    /// The last error message, when the run failed.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// A snapshot for the console.
    pub fn snapshot(&self) -> CalibrationSnapshot {
        CalibrationSnapshot {
            phase: self.phase,
            target: self.target,
            total: self.grid.len(),
            completed: self.completed,
            collected: self.count,
            report: self.full_report,
            quick_report: self.quick_report,
            fitted: self.calibration.is_fitted(),
        }
    }

    /// The scene for the current phase.
    ///
    /// Returns an empty scene when the run is idle or terminal; the caller
    /// decides whether to restore the previous experiment scene.
    pub fn scene(&self) -> Scene {
        match self.phase {
            CalibrationPhase::Settle | CalibrationPhase::Collect => {
                let text = match self.target {
                    Some(index) => format!(
                        "Look at the orange target ({}/{}) · press Esc in the console to cancel",
                        index + 1,
                        self.grid.len()
                    ),
                    None => "Look at the orange target".to_owned(),
                };
                self.grid.scene_with_text(self.target, text)
            }
            CalibrationPhase::FullReport => {
                let text = self
                    .full_report
                    .map(|report| {
                        format!(
                            "Calibration complete · RMSE {:.1} px · {} samples",
                            report.rmse, report.samples
                        )
                    })
                    .unwrap_or_else(|| "Calibration complete".to_owned());
                self.grid.scene_with_text(None, text)
            }
            CalibrationPhase::QuickSettle | CalibrationPhase::QuickCollect => {
                self.grid.quick_scene(
                    self.target,
                    self.quick_background,
                    "Quick recalibration · look at the orange center target",
                )
            }
            CalibrationPhase::QuickReport => {
                let text = self
                    .quick_report
                    .map(|report| {
                        format!(
                            "Quick recalibration · RMSE {:.1} px · {} samples",
                            report.rmse, report.samples
                        )
                    })
                    .unwrap_or_else(|| "Quick recalibration complete".to_owned());
                self.grid.quick_scene(None, self.quick_background, text)
            }
            CalibrationPhase::Idle
            | CalibrationPhase::Done
            | CalibrationPhase::Failed
            | CalibrationPhase::Cancelled => Scene::new(),
        }
    }

    fn collect(&mut self, features: Option<QuadBrightness>) {
        if let Some(features) = features {
            for (sum, value) in self.sums.iter_mut().zip(features.to_array()) {
                *sum += f64::from(value);
            }
            self.count += 1;
        }
    }

    fn finish_target(&mut self, now: SessionTime) -> Result<(), CalibrationError> {
        let index = self.target.ok_or(CalibrationError::MissingTarget(0))?;
        if self.count == 0 {
            return Err(self.fail(CalibrationError::NoFrames {
                target: index + 1,
                total: self.grid.len(),
            }));
        }
        let target = self
            .grid
            .target(index)
            .ok_or(CalibrationError::MissingTarget(index))?;
        let features = mean(self.sums, self.count);
        self.calibration.record(
            GazePoint::new(target.position_px.x, target.position_px.y),
            features,
        );
        self.completed += 1;
        self.sums = [0.0; 4];
        self.count = 0;

        if index + 1 < self.grid.len() {
            self.target = Some(index + 1);
            self.phase = CalibrationPhase::Settle;
            self.since = now;
            return Ok(());
        }

        match self.calibration.fit() {
            Ok(report) => {
                self.full_report = Some(report);
                self.quick_report = None;
                self.target = None;
                self.phase = CalibrationPhase::FullReport;
                self.since = now;
                self.backup = None;
                Ok(())
            }
            Err(error) => Err(self.fail(CalibrationError::Model(error))),
        }
    }

    fn finish_quick(&mut self, now: SessionTime) -> Result<(), CalibrationError> {
        let index = self.grid.len() / 2;
        if self.count == 0 {
            return Err(self.fail(CalibrationError::NoFrames {
                target: 1,
                total: 1,
            }));
        }
        let target = self
            .grid
            .target(index)
            .ok_or(CalibrationError::MissingTarget(index))?;
        let features = mean(self.sums, self.count);
        self.calibration.record(
            GazePoint::new(target.position_px.x, target.position_px.y),
            features,
        );
        self.completed += 1;

        match self.calibration.fit() {
            Ok(report) => {
                self.quick_report = Some(report);
                self.target = None;
                self.phase = CalibrationPhase::QuickReport;
                self.since = now;
                Ok(())
            }
            Err(error) => Err(self.fail(CalibrationError::Model(error))),
        }
    }

    fn fail(&mut self, error: CalibrationError) -> CalibrationError {
        if let Some(backup) = self.backup.take() {
            self.calibration = backup;
        }
        self.phase = CalibrationPhase::Failed;
        self.target = None;
        self.sums = [0.0; 4];
        self.count = 0;
        self.quick_background = None;
        self.error = Some(error.to_string());
        error
    }
}

fn mean(sums: [f64; 4], count: usize) -> QuadBrightness {
    let count = count.max(1) as f64;
    QuadBrightness::new(
        (sums[0] / count) as f32,
        (sums[1] / count) as f32,
        (sums[2] / count) as f32,
        (sums[3] / count) as f32,
    )
}

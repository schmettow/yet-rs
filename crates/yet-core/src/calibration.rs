// SPDX-License-Identifier: GPL-3.0-only

//! Calibration targets on the presenter monitor.
//!
//! The grid is a pure geometry description. It produces [`Scene`]s for a
//! presenter and normalized targets for [`CalibrationSample`]s; it never reads
//! a camera and never draws by itself.
//!
//! [`CalibrationSample`]: crate::tracking::CalibrationSample

use yet_present::{Color, PointPx, Scene};

use crate::layout::BACKGROUND_COLOR;
use crate::tracking::GazePoint;

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

        Self { targets }
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
        let mut scene = Scene::with_capacity(self.targets.len() + 1);
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
}

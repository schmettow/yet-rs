// SPDX-License-Identifier: GPL-3.0-only

//! Eye-tracking data types that do not depend on a camera or UI.
//!
//! The full feature extraction and regression pipeline arrives in Sprint 4.
//! These types are defined now so the engine protocol can carry calibration
//! samples without a later breaking change.

/// Mean brightness of the four quadrants of an eye region.
///
/// The canonical order is `(nw, ne, sw, se)`. This differs from the Python
/// prototype's CSV column order, which is `(nw, sw, ne, se)`; conversion belongs
/// at the data-import boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuadBrightness {
    /// North-west (top-left) mean.
    pub nw: f32,
    /// North-east (top-right) mean.
    pub ne: f32,
    /// South-west (bottom-left) mean.
    pub sw: f32,
    /// South-east (bottom-right) mean.
    pub se: f32,
}

impl QuadBrightness {
    /// Create quadrant means in canonical order.
    pub const fn new(nw: f32, ne: f32, sw: f32, se: f32) -> Self {
        Self { nw, ne, sw, se }
    }
}

/// A gaze position predicted by a model.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GazePoint {
    /// Horizontal position.
    pub x: f32,
    /// Vertical position.
    pub y: f32,
}

impl GazePoint {
    /// Create a gaze point.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// One calibration observation: features paired with a known target.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CalibrationSample {
    /// Extracted eye features.
    pub features: QuadBrightness,
    /// Known target position.
    pub target: GazePoint,
}

impl CalibrationSample {
    /// Create a calibration sample.
    pub const fn new(features: QuadBrightness, target: GazePoint) -> Self {
        Self { features, target }
    }
}

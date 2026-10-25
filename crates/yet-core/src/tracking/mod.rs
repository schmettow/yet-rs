// SPDX-License-Identifier: GPL-3.0-only

//! Eye-tracking data types, features and the linear gaze model.
//!
//! This module is independent of any camera, image decoder or UI. A camera
//! adapter converts its buffer into a [`GrayFrame`]; [`quad_brightness`]
//! reduces a fixed [`EyeRegion`] to four means; a [`GazeModel`] maps those to a
//! [`GazePoint`] in monitor pixels.
//!
//! The canonical quadrant order is `(nw, ne, sw, se)`. The Python prototype's
//! CSV column order is `(nw, sw, ne, se)` and its own labels are swapped;
//! conversion belongs at the data-import boundary.

mod buffer;
mod calibration;
mod features;
mod frame;
mod model;

pub use buffer::CalibrationBuffer;
pub use calibration::{Calibration, CalibrationReport};
pub use features::{quad_brightness, QuadBrightness};
pub use frame::{rec601_luma, EyeRegion, FrameError, GrayFrame};
pub use model::{
    GazeModel, LinearGazeModel, LinearParameters, ModelError, MIN_CALIBRATION_SAMPLES,
};

/// A gaze position predicted by a model, or a calibration target.
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
    /// Known target position, in monitor pixels.
    pub target: GazePoint,
}

impl CalibrationSample {
    /// Create a calibration sample.
    pub const fn new(features: QuadBrightness, target: GazePoint) -> Self {
        Self { features, target }
    }
}

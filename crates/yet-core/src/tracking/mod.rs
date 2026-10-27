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

#[cfg(feature = "yet-zero")]
mod buffer;
#[cfg(feature = "yet-zero")]
mod calibration;
mod features;
mod frame;
#[cfg(feature = "yet-zero")]
mod model;

pub use crate::gaze::{
    BodyPose, FeatureLayout, FeatureVector, GazeInput, GazePoint, GazePredictor, HeadPose,
    LinearPredictor, ModelParameters, PoseSample, PredictionError, MAX_FEATURES,
    MODEL_FORMAT_VERSION, OCTA_LAYOUT, QUAD_LAYOUT,
};
#[cfg(feature = "yet-zero")]
pub use buffer::CalibrationBuffer;
#[cfg(feature = "yet-zero")]
pub use calibration::{Calibration, CalibrationReport};
pub use features::{quad_brightness, FeatureExtractor, QuadBrightness, QuadExtractor};
pub use frame::{rec601_luma, validate_eye_region, EyeRegion, FrameError, GrayFrame};
#[cfg(feature = "yet-zero")]
pub use model::{
    minimum_samples, GazeModel, GazeTrainer, LinearGazeModel, LinearParameters, ModelError,
    MIN_CALIBRATION_SAMPLES,
};

/// One calibration observation: features paired with a known target.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CalibrationSample {
    /// Extracted eye features in layout order.
    pub features: FeatureVector,
    /// Known target position, in monitor pixels.
    pub target: GazePoint,
    /// Head/body pose at the sample time, when available.
    pub pose: Option<PoseSample>,
}

/// The future-facing name for a calibration observation.
pub type CalibrationObservation = CalibrationSample;

impl CalibrationSample {
    /// Create a v1 Quadbright sample; pose is absent.
    #[cfg(feature = "yet-zero")]
    pub fn new(features: QuadBrightness, target: GazePoint) -> Self {
        Self {
            features: FeatureVector::from(features),
            target,
            pose: None,
        }
    }

    /// Create a sample from a generic feature vector.
    pub fn from_features(features: FeatureVector, target: GazePoint) -> Self {
        Self {
            features,
            target,
            pose: None,
        }
    }

    /// Add a pose observation.
    pub const fn with_pose(mut self, pose: PoseSample) -> Self {
        self.pose = Some(pose);
        self
    }
}

// SPDX-License-Identifier: GPL-3.0-only

//! Calibration buffer plus model, for full and quick recalibration.

use super::{
    CalibrationBuffer, CalibrationSample, GazeModel, GazePoint, LinearGazeModel, LinearParameters,
    ModelError, QuadBrightness,
};

/// Summary of a calibration fit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalibrationReport {
    /// Number of samples used for the fit.
    pub samples: usize,
    /// Euclidean RMS calibration error, in target units.
    pub rmse: f32,
}

/// Collects calibration observations and fits a linear gaze model.
///
/// A full calibration records all grid targets, then calls [`Self::fit`]. A
/// quick recalibration records one or more new samples and calls [`Self::fit`]
/// again; the older samples are kept and the model is replaced. No session
/// restart is required.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Calibration {
    buffer: CalibrationBuffer,
    model: LinearGazeModel,
}

impl Calibration {
    /// Create an empty calibration.
    pub const fn new() -> Self {
        Self {
            buffer: CalibrationBuffer::new(),
            model: LinearGazeModel::new(),
        }
    }

    /// Add one observation at a known target, in monitor pixels.
    pub fn record(&mut self, target: GazePoint, features: QuadBrightness) {
        self.buffer.push(CalibrationSample::new(features, target));
    }

    /// Fit or refit the model to every recorded sample.
    ///
    /// A failed fit leaves the previous model and the samples intact.
    pub fn fit(&mut self) -> Result<CalibrationReport, ModelError> {
        self.buffer.fit(&mut self.model)?;
        let rmse = self
            .buffer
            .rmse(&self.model)?
            .ok_or(ModelError::NotFitted)?;
        Ok(CalibrationReport {
            samples: self.buffer.len(),
            rmse,
        })
    }

    /// Predict a gaze position in monitor pixels.
    pub fn predict(&self, features: QuadBrightness) -> Result<GazePoint, ModelError> {
        self.model.predict(features)
    }

    /// Whether a successful fit is active.
    pub fn is_fitted(&self) -> bool {
        self.model.is_fitted()
    }

    /// The recorded observations.
    pub fn buffer(&self) -> &CalibrationBuffer {
        &self.buffer
    }

    /// Fitted coefficients and intercept, when fitted.
    pub fn parameters(&self) -> Option<LinearParameters> {
        self.model.parameters()
    }

    /// Clear both the observations and the fitted model.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.model.reset();
    }
}

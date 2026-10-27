// SPDX-License-Identifier: GPL-3.0-only

//! A calibration sample buffer and error metric.

use alloc::vec::Vec;

use super::{
    CalibrationSample, GazeInput, GazeModel, GazePredictor, GazeTrainer, ModelError,
    PredictionError,
};

/// Calibration observations collected for one or more fits.
///
/// Quick recalibration refits from this buffer. The buffer does not deduplicate
/// or filter; the calibration flow decides when a sample is stable enough.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalibrationBuffer {
    samples: Vec<CalibrationSample>,
}

impl CalibrationBuffer {
    /// Create an empty buffer.
    pub const fn new() -> Self {
        Self {
            samples: Vec::new(),
        }
    }

    /// Create an empty buffer with reserved capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            samples: Vec::with_capacity(capacity),
        }
    }

    /// Add one observation.
    pub fn push(&mut self, sample: CalibrationSample) {
        self.samples.push(sample);
    }

    /// The observations in insertion order.
    pub fn samples(&self) -> &[CalibrationSample] {
        &self.samples
    }

    /// Number of observations.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether no observations have been collected.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Remove all observations.
    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// Fit `model` to every buffered observation.
    pub fn fit(&self, model: &mut dyn GazeModel) -> Result<(), ModelError> {
        GazeTrainer::fit(model, &self.samples)
    }

    /// Root-mean-square Euclidean calibration error, in target units.
    ///
    /// Returns `Ok(None)` for an empty buffer. A model that has not been fitted
    /// returns [`ModelError::NotFitted`].
    pub fn rmse(&self, model: &dyn GazeModel) -> Result<Option<f32>, ModelError> {
        if self.samples.is_empty() {
            return Ok(None);
        }

        let mut squared_error = 0.0_f64;
        for sample in &self.samples {
            let predicted = GazePredictor::predict(
                model,
                GazeInput::new(sample.features.as_slice(), sample.pose.as_ref()),
            )
            .map_err(|error| match error {
                PredictionError::NotFitted => ModelError::NotFitted,
                other => ModelError::Prediction(other),
            })?;
            let dx = f64::from(predicted.x) - f64::from(sample.target.x);
            let dy = f64::from(predicted.y) - f64::from(sample.target.y);
            squared_error += dx * dx + dy * dy;
        }

        let mean_squared = squared_error / self.samples.len() as f64;
        Ok(Some(libm::sqrt(mean_squared) as f32))
    }
}

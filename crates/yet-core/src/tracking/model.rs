// SPDX-License-Identifier: GPL-3.0-only

//! The 4-to-2 linear gaze model.

use nalgebra::DMatrix;
use thiserror::Error;

use super::{CalibrationSample, GazePoint, QuadBrightness};

/// Minimum number of samples for a uniquely determined 4-to-2 fit.
pub const MIN_CALIBRATION_SAMPLES: usize = 4;

/// Errors from fitting or using a gaze model.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ModelError {
    /// Too few samples were supplied.
    #[error("at least {required} calibration samples are required, got {actual}")]
    NotEnoughSamples {
        /// Minimum number of samples.
        required: usize,
        /// Supplied number of samples.
        actual: usize,
    },
    /// Prediction was attempted before a successful fit.
    #[error("the gaze model is not fitted")]
    NotFitted,
    /// The least-squares solver did not converge.
    #[error("the least-squares solver failed")]
    Solver,
}

/// A swappable 4-to-2 gaze model.
pub trait GazeModel {
    /// Replace the model with a fit to `samples`.
    ///
    /// A failed fit leaves a previously fitted model unchanged. Fitting is
    /// repeatable and cheap enough for quick recalibration.
    fn fit(&mut self, samples: &[CalibrationSample]) -> Result<(), ModelError>;

    /// Predict a gaze position in the same units as the calibration targets.
    fn predict(&self, features: QuadBrightness) -> Result<GazePoint, ModelError>;

    /// Whether a successful fit is active.
    fn is_fitted(&self) -> bool;

    /// Clear the calibration for a new session.
    fn reset(&mut self);
}

/// Fitted linear parameters, in canonical feature order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearParameters {
    /// Two rows of four coefficients, in canonical feature order.
    pub coefficients: [[f64; 4]; 2],
    /// One intercept per output coordinate.
    pub intercept: [f64; 2],
}

/// Ordinary least squares from four quadrant means to two coordinates.
///
/// Features and targets are centered before the solve, as scikit-learn's
/// `LinearRegression(fit_intercept=True)` does. The solve uses an SVD rather
/// than normal equations because quadrant means are strongly collinear.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinearGazeModel {
    fit: Option<LinearParameters>,
}

impl LinearGazeModel {
    /// Create an unfitted model.
    pub const fn new() -> Self {
        Self { fit: None }
    }

    /// The fitted parameters, in the units of the calibration targets.
    pub fn parameters(&self) -> Option<LinearParameters> {
        self.fit
    }

    fn solve(samples: &[CalibrationSample]) -> Result<LinearParameters, ModelError> {
        if samples.len() < MIN_CALIBRATION_SAMPLES {
            return Err(ModelError::NotEnoughSamples {
                required: MIN_CALIBRATION_SAMPLES,
                actual: samples.len(),
            });
        }

        let count = samples.len() as f64;
        let mut feature_sums = [0.0_f64; 4];
        let mut target_sums = [0.0_f64; 2];
        for sample in samples {
            for (sum, value) in feature_sums.iter_mut().zip(sample.features.to_array()) {
                *sum += f64::from(value);
            }
            target_sums[0] += f64::from(sample.target.x);
            target_sums[1] += f64::from(sample.target.y);
        }
        let means = feature_sums.map(|sum| sum / count);
        let target_means = target_sums.map(|sum| sum / count);

        let mut design = DMatrix::<f64>::zeros(samples.len(), 4);
        let mut targets = DMatrix::<f64>::zeros(samples.len(), 2);
        for (row, sample) in samples.iter().enumerate() {
            for (column, value) in sample.features.to_array().into_iter().enumerate() {
                design[(row, column)] = f64::from(value) - means[column];
            }
            targets[(row, 0)] = f64::from(sample.target.x) - target_means[0];
            targets[(row, 1)] = f64::from(sample.target.y) - target_means[1];
        }

        let svd = design.svd(true, true);
        let largest = svd.singular_values.iter().copied().fold(0.0_f64, f64::max);
        let epsilon = f64::EPSILON * samples.len().max(4) as f64 * largest;
        let beta = svd
            .solve(&targets, epsilon)
            .map_err(|_| ModelError::Solver)?;

        let coefficients = [
            [beta[(0, 0)], beta[(1, 0)], beta[(2, 0)], beta[(3, 0)]],
            [beta[(0, 1)], beta[(1, 1)], beta[(2, 1)], beta[(3, 1)]],
        ];
        let intercept = [
            target_means[0] - dot(coefficients[0], means),
            target_means[1] - dot(coefficients[1], means),
        ];

        Ok(LinearParameters {
            coefficients,
            intercept,
        })
    }
}

impl GazeModel for LinearGazeModel {
    fn fit(&mut self, samples: &[CalibrationSample]) -> Result<(), ModelError> {
        let fit = Self::solve(samples)?;
        self.fit = Some(fit);
        Ok(())
    }

    fn predict(&self, features: QuadBrightness) -> Result<GazePoint, ModelError> {
        let fit = self.fit.as_ref().ok_or(ModelError::NotFitted)?;
        let features = features.to_array().map(f64::from);

        Ok(GazePoint::new(
            (fit.intercept[0] + dot(fit.coefficients[0], features)) as f32,
            (fit.intercept[1] + dot(fit.coefficients[1], features)) as f32,
        ))
    }

    fn is_fitted(&self) -> bool {
        self.fit.is_some()
    }

    fn reset(&mut self) {
        self.fit = None;
    }
}

fn dot(coefficients: [f64; 4], features: [f64; 4]) -> f64 {
    coefficients
        .into_iter()
        .zip(features)
        .map(|(coefficient, feature)| coefficient * feature)
        .sum()
}

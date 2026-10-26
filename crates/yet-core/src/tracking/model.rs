// SPDX-License-Identifier: GPL-3.0-only

//! The v1 glance model: a linear map from features to screen pixels.
//!
//! Training is host-side and may use `nalgebra`; prediction is a plain
//! fixed-size linear combination. The deployable [`ModelParameters`] block is
//! `f32`-only and can be consumed by [`yet_gaze::LinearPredictor`] on a
//! microcontroller. The v1 implementation remains a four-feature glance model,
//! but the solver and parameters are feature-count aware.

use nalgebra::DMatrix;
use thiserror::Error;

use super::{
    CalibrationSample, GazeInput, GazePoint, GazePredictor, ModelParameters, PredictionError,
    QuadBrightness, MAX_FEATURES, MODEL_FORMAT_VERSION,
};

/// Minimum number of samples accepted by the v1 four-feature fit.
pub const MIN_CALIBRATION_SAMPLES: usize = 4;

/// Minimum sample count for a full-rank fit of `feature_count` features.
///
/// The v1 four-feature path keeps its historical minimum of four (the SVD
/// returns a minimum-norm solution for the rank-deficient case). Larger layouts,
/// such as 8-feature Octabright, require `feature_count + 1`.
pub const fn minimum_samples(feature_count: usize) -> usize {
    if feature_count <= 4 {
        MIN_CALIBRATION_SAMPLES
    } else {
        feature_count + 1
    }
}

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
    /// Samples disagree about their feature count.
    #[error("calibration samples have {expected} and {actual} features; all must match")]
    InconsistentFeatures {
        /// First sample's feature count.
        expected: usize,
        /// Conflicting feature count.
        actual: usize,
    },
    /// The samples contain no features.
    #[error("calibration samples contain no features")]
    NoFeatures,
    /// The least-squares solver did not converge.
    #[error("the least-squares solver failed")]
    Solver,
    /// Portable prediction failed.
    #[error(transparent)]
    Prediction(#[from] PredictionError),
}

/// A model that can be trained on the host.
pub trait GazeTrainer {
    /// Replace the model with a fit to `samples`.
    ///
    /// A failed fit leaves a previously fitted model unchanged. Fitting is
    /// repeatable and cheap enough for quick recalibration.
    fn fit(&mut self, samples: &[CalibrationSample]) -> Result<(), ModelError>;

    /// Clear the fitted parameters for a new session.
    fn reset(&mut self);
}

/// The combined v1 model contract: train on the host, predict anywhere.
pub trait GazeModel: GazeTrainer + GazePredictor {}

impl<T: GazeTrainer + GazePredictor> GazeModel for T {}

/// Fitted linear parameters, in canonical feature order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearParameters {
    /// Number of active features.
    pub feature_count: usize,
    /// Two rows of coefficients, in canonical feature order.
    pub coefficients: [[f64; MAX_FEATURES]; 2],
    /// One intercept per output coordinate.
    pub intercept: [f64; 2],
}

impl LinearParameters {
    /// Convert to the plain `f32` block deployed to a device.
    pub fn to_model_parameters(&self) -> ModelParameters {
        let mut coefficients = [[0.0_f32; MAX_FEATURES]; 2];
        for (row, values) in self.coefficients.iter().enumerate() {
            for (index, value) in values.iter().take(self.feature_count).enumerate() {
                coefficients[row][index] = *value as f32;
            }
        }
        ModelParameters {
            version: MODEL_FORMAT_VERSION,
            feature_count: self.feature_count as u8,
            coefficients,
            intercept: self.intercept.map(|value| value as f32),
        }
    }
}

impl From<&LinearParameters> for ModelParameters {
    fn from(parameters: &LinearParameters) -> Self {
        parameters.to_model_parameters()
    }
}

/// Ordinary least squares from features to two coordinates.
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

    /// The fitted parameters in the portable deployment format.
    pub fn model_parameters(&self) -> Option<ModelParameters> {
        self.fit.as_ref().map(LinearParameters::to_model_parameters)
    }

    /// Convenience fit for the v1 path; the trait form is available too.
    pub fn fit(&mut self, samples: &[CalibrationSample]) -> Result<(), ModelError> {
        <Self as GazeTrainer>::fit(self, samples)
    }

    /// Convenience prediction from four named quadrant means.
    pub fn predict(&self, features: QuadBrightness) -> Result<GazePoint, ModelError> {
        let fit = self.fit.as_ref().ok_or(ModelError::NotFitted)?;
        if fit.feature_count != 4 {
            return Err(ModelError::Prediction(PredictionError::FeatureCount {
                expected: fit.feature_count,
                actual: 4,
            }));
        }
        let values = features.to_array().map(f64::from);

        Ok(GazePoint::new(
            (fit.intercept[0] + dot(&fit.coefficients[0][..4], &values)) as f32,
            (fit.intercept[1] + dot(&fit.coefficients[1][..4], &values)) as f32,
        ))
    }

    /// Convenience reset; the trait form is available too.
    pub fn reset(&mut self) {
        <Self as GazeTrainer>::reset(self);
    }

    /// Whether a successful fit is active.
    pub fn is_fitted(&self) -> bool {
        self.fit.is_some()
    }

    fn solve(samples: &[CalibrationSample]) -> Result<LinearParameters, ModelError> {
        let feature_count = samples.first().map_or(0, |sample| sample.features.len());
        let required = minimum_samples(feature_count);
        if samples.len() < required {
            return Err(ModelError::NotEnoughSamples {
                required,
                actual: samples.len(),
            });
        }
        if feature_count == 0 {
            return Err(ModelError::NoFeatures);
        }
        for sample in samples {
            if sample.features.len() != feature_count {
                return Err(ModelError::InconsistentFeatures {
                    expected: feature_count,
                    actual: sample.features.len(),
                });
            }
        }

        let count = samples.len() as f64;
        let mut feature_sums = [0.0_f64; MAX_FEATURES];
        let mut target_sums = [0.0_f64; 2];
        for sample in samples {
            for (sum, value) in feature_sums.iter_mut().zip(sample.features.as_slice()) {
                *sum += f64::from(*value);
            }
            target_sums[0] += f64::from(sample.target.x);
            target_sums[1] += f64::from(sample.target.y);
        }
        let means = feature_sums.map(|sum| sum / count);
        let target_means = target_sums.map(|sum| sum / count);

        let mut design = DMatrix::<f64>::zeros(samples.len(), feature_count);
        let mut targets = DMatrix::<f64>::zeros(samples.len(), 2);
        for (row, sample) in samples.iter().enumerate() {
            for (column, value) in sample.features.as_slice().iter().enumerate() {
                design[(row, column)] = f64::from(*value) - means[column];
            }
            targets[(row, 0)] = f64::from(sample.target.x) - target_means[0];
            targets[(row, 1)] = f64::from(sample.target.y) - target_means[1];
        }

        let svd = design.svd(true, true);
        let largest = svd.singular_values.iter().copied().fold(0.0_f64, f64::max);
        let epsilon = f64::EPSILON * samples.len().max(feature_count + 1) as f64 * largest;
        let beta = svd
            .solve(&targets, epsilon)
            .map_err(|_| ModelError::Solver)?;

        let mut coefficients = [[0.0_f64; MAX_FEATURES]; 2];
        for row in 0..2 {
            for column in 0..feature_count {
                coefficients[row][column] = beta[(column, row)];
            }
        }
        let intercept = [
            target_means[0] - dot(&coefficients[0][..feature_count], &means[..feature_count]),
            target_means[1] - dot(&coefficients[1][..feature_count], &means[..feature_count]),
        ];

        Ok(LinearParameters {
            feature_count,
            coefficients,
            intercept,
        })
    }
}

impl GazeTrainer for LinearGazeModel {
    fn fit(&mut self, samples: &[CalibrationSample]) -> Result<(), ModelError> {
        let fit = Self::solve(samples)?;
        self.fit = Some(fit);
        Ok(())
    }

    fn reset(&mut self) {
        self.fit = None;
    }
}

impl GazePredictor for LinearGazeModel {
    fn predict(&self, input: GazeInput<'_>) -> Result<GazePoint, PredictionError> {
        let fit = self.fit.as_ref().ok_or(PredictionError::NotFitted)?;
        if input.features.len() != fit.feature_count {
            return Err(PredictionError::FeatureCount {
                expected: fit.feature_count,
                actual: input.features.len(),
            });
        }
        let mut output = fit.intercept;
        for (index, feature) in input.features.iter().copied().enumerate() {
            let feature = f64::from(feature);
            output[0] += fit.coefficients[0][index] * feature;
            output[1] += fit.coefficients[1][index] * feature;
        }
        Ok(GazePoint::new(output[0] as f32, output[1] as f32))
    }

    fn is_fitted(&self) -> bool {
        self.fit.is_some()
    }
}

fn dot(coefficients: &[f64], features: &[f64]) -> f64 {
    coefficients
        .iter()
        .zip(features)
        .map(|(coefficient, feature)| coefficient * feature)
        .sum()
}

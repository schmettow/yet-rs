// SPDX-License-Identifier: GPL-3.0-only

//! Portable feature and gaze-prediction primitives.
//!
//! This crate is `no_std`, allocation-free and free of UI, image, time and
//! `nalgebra` types. A host fits a model with `yet-core`'s trainer, converts
//! the result to [`ModelParameters`] and deploys it here; a microcontroller
//! only needs [`LinearPredictor`].
//!
//! v1 uses the four-feature [`QUAD_LAYOUT`]. [`OCTA_LAYOUT`] is the future
//! binocular layout (two eye cameras, four quadrants each). The layout is
//! metadata: the predictor consumes a feature slice and its parameter block,
//! not a fixed four-field struct.

#![no_std]

#[cfg(test)]
extern crate std;

use core::fmt;

/// Maximum number of features a portable model can consume.
///
/// Four features are Quadbright, eight are Octabright; the headroom covers a
/// future combined layout without changing the fixed-capacity vector.
pub const MAX_FEATURES: usize = 16;

/// Version written into deployable parameter blocks.
pub const MODEL_FORMAT_VERSION: u8 = 1;

/// Names and order of the features a model expects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureLayout {
    /// Short layout name, e.g. `"quad"` or `"octa"`.
    pub name: &'static str,
    /// Feature labels in canonical order.
    pub labels: &'static [&'static str],
}

impl FeatureLayout {
    /// Create a layout from a static label list.
    pub const fn new(name: &'static str, labels: &'static [&'static str]) -> Self {
        Self { name, labels }
    }

    /// Number of features.
    pub const fn len(&self) -> usize {
        self.labels.len()
    }

    /// Whether the layout has no features.
    pub const fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    /// Position of a label in canonical order.
    pub fn index_of(&self, label: &str) -> Option<usize> {
        self.labels.iter().position(|candidate| *candidate == label)
    }
}

/// The v1 four-quadrant brightness layout.
pub const QUAD_LAYOUT: FeatureLayout = FeatureLayout::new("quad", &["nw", "ne", "sw", "se"]);

/// The future binocular layout: left eye quadrants, then right eye quadrants.
pub const OCTA_LAYOUT: FeatureLayout = FeatureLayout::new(
    "octa",
    &[
        "left_nw", "left_ne", "left_sw", "left_se", "right_nw", "right_ne", "right_sw", "right_se",
    ],
);

/// A fixed-capacity feature vector.
///
/// Four quadrant means are compressed to this vector on the v1 path; the
/// capacity allows an 8-feature Octabright vector and future combinations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FeatureVector {
    values: [f32; MAX_FEATURES],
    len: usize,
}

impl FeatureVector {
    /// Copy a feature slice, rejecting more than [`MAX_FEATURES`] values.
    pub fn new(values: &[f32]) -> Option<Self> {
        if values.len() > MAX_FEATURES {
            return None;
        }
        let mut data = [0.0; MAX_FEATURES];
        data[..values.len()].copy_from_slice(values);
        Some(Self {
            values: data,
            len: values.len(),
        })
    }

    /// Build the four-feature Quadbright vector.
    pub const fn from_quad(values: [f32; 4]) -> Self {
        let mut data = [0.0; MAX_FEATURES];
        data[0] = values[0];
        data[1] = values[1];
        data[2] = values[2];
        data[3] = values[3];
        Self {
            values: data,
            len: 4,
        }
    }

    /// Number of active features.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the vector has no features.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Active features in canonical order.
    pub fn as_slice(&self) -> &[f32] {
        &self.values[..self.len]
    }

    /// One feature value, when the index is active.
    pub fn get(&self, index: usize) -> Option<f32> {
        (index < self.len).then(|| self.values[index])
    }
}

impl Default for FeatureVector {
    fn default() -> Self {
        Self {
            values: [0.0; MAX_FEATURES],
            len: 0,
        }
    }
}

/// A gaze position in whichever output frame the model uses.
///
/// v1 and the normal Yet workflow use screen pixels, because fixed AOIs are
/// defined most easily in that frame. The pipeline may compute in polar or
/// world coordinates internally when head movement is involved; the final
/// estimate is still projected to pixels.
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

/// Head orientation in degrees, from a head-mounted IMU.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeadPose {
    /// Yaw in degrees.
    pub yaw_deg: f32,
    /// Pitch in degrees.
    pub pitch_deg: f32,
    /// Roll in degrees.
    pub roll_deg: f32,
}

impl HeadPose {
    /// Create a head pose.
    pub const fn new(yaw_deg: f32, pitch_deg: f32, roll_deg: f32) -> Self {
        Self {
            yaw_deg,
            pitch_deg,
            roll_deg,
        }
    }
}

/// Body orientation in degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BodyPose {
    /// Yaw in degrees.
    pub yaw_deg: f32,
    /// Pitch in degrees.
    pub pitch_deg: f32,
    /// Roll in degrees.
    pub roll_deg: f32,
}

impl BodyPose {
    /// Create a body pose.
    pub const fn new(yaw_deg: f32, pitch_deg: f32, roll_deg: f32) -> Self {
        Self {
            yaw_deg,
            pitch_deg,
            roll_deg,
        }
    }
}

/// One timestamped pose observation.
///
/// `at_micros` is monotonic time in microseconds. The host converts its
/// `SessionTime`; a microcontroller can use its own monotonic counter.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PoseSample {
    /// Monotonic sample time in microseconds.
    pub at_micros: u64,
    /// Head orientation, when a head IMU is present.
    pub head: Option<HeadPose>,
    /// Body orientation, when a body sensor is present.
    pub body: Option<BodyPose>,
}

impl PoseSample {
    /// Create a pose sample.
    pub const fn new(at_micros: u64, head: Option<HeadPose>, body: Option<BodyPose>) -> Self {
        Self {
            at_micros,
            head,
            body,
        }
    }
}

/// The input to a predictor.
///
/// `pose = None` is the v1 glance model. A future gaze model consumes the same
/// input with a pose; call sites do not change.
#[derive(Clone, Copy, Debug)]
pub struct GazeInput<'a> {
    /// Active features in the model's layout order.
    pub features: &'a [f32],
    /// Optional head/body pose.
    pub pose: Option<&'a PoseSample>,
}

impl<'a> GazeInput<'a> {
    /// Create a gaze input.
    pub const fn new(features: &'a [f32], pose: Option<&'a PoseSample>) -> Self {
        Self { features, pose }
    }

    /// Create an eye-only input.
    pub const fn glance(features: &'a [f32]) -> Self {
        Self {
            features,
            pose: None,
        }
    }
}

/// A deployable, plain, versioned linear parameter block.
///
/// Coefficients are stored in feature-major order for two output coordinates.
/// The block contains only `f32` values and fixed-size arrays; it can be written
/// to flash and read by a device without `nalgebra` or an allocator.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelParameters {
    /// Format version, currently [`MODEL_FORMAT_VERSION`].
    pub version: u8,
    /// Number of active features (4 for Quadbright, 8 for Octabright).
    pub feature_count: u8,
    /// Two rows of [`MAX_FEATURES`] coefficients, in canonical feature order.
    pub coefficients: [[f32; MAX_FEATURES]; 2],
    /// One intercept per output coordinate.
    pub intercept: [f32; 2],
}

impl Default for ModelParameters {
    fn default() -> Self {
        Self {
            version: MODEL_FORMAT_VERSION,
            feature_count: 0,
            coefficients: [[0.0; MAX_FEATURES]; 2],
            intercept: [0.0; 2],
        }
    }
}

impl ModelParameters {
    /// Build a parameter block, rejecting too many features.
    pub fn new(
        feature_count: usize,
        coefficients: [[f32; MAX_FEATURES]; 2],
        intercept: [f32; 2],
    ) -> Option<Self> {
        (feature_count <= MAX_FEATURES).then_some(Self {
            version: MODEL_FORMAT_VERSION,
            feature_count: feature_count as u8,
            coefficients,
            intercept,
        })
    }
}

/// Why a portable predictor cannot return a gaze point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PredictionError {
    /// The model has no fitted parameters.
    NotFitted,
    /// The parameter block declares an unusable feature count.
    InvalidParameters {
        /// Declared feature count.
        feature_count: u8,
    },
    /// The feature slice does not match the parameter block.
    FeatureCount {
        /// Number of features the model expects.
        expected: usize,
        /// Number of features supplied.
        actual: usize,
    },
}

impl fmt::Display for PredictionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFitted => formatter.write_str("the gaze model is not fitted"),
            Self::InvalidParameters { feature_count } => write!(
                formatter,
                "the model declares {feature_count} features, but at most {MAX_FEATURES} are supported"
            ),
            Self::FeatureCount { expected, actual } => write!(
                formatter,
                "the model expects {expected} features, got {actual}"
            ),
        }
    }
}

impl core::error::Error for PredictionError {}

/// A model that can predict without training or host resources.
pub trait GazePredictor {
    /// Predict a gaze point from features and optional pose.
    fn predict(&self, input: GazeInput<'_>) -> Result<GazePoint, PredictionError>;

    /// Whether fitted parameters are available.
    fn is_fitted(&self) -> bool;
}

/// A portable linear predictor over [`ModelParameters`].
///
/// This is the shape an ESP32-C implementation can use: four multiply-adds per
/// output, no allocation and no floating-point type wider than `f32`. If the
/// selected ESP32-C variant has no hardware FPU, a fixed-point implementation
/// behind [`GazePredictor`] can replace it without changing callers.
#[derive(Clone, Debug, PartialEq)]
pub struct LinearPredictor {
    parameters: ModelParameters,
}

impl LinearPredictor {
    /// Create a predictor from a parameter block.
    pub const fn new(parameters: ModelParameters) -> Self {
        Self { parameters }
    }

    /// The deployed parameter block.
    pub const fn parameters(&self) -> &ModelParameters {
        &self.parameters
    }
}

impl GazePredictor for LinearPredictor {
    fn predict(&self, input: GazeInput<'_>) -> Result<GazePoint, PredictionError> {
        let parameters = &self.parameters;
        if parameters.version != MODEL_FORMAT_VERSION {
            return Err(PredictionError::NotFitted);
        }
        let expected = usize::from(parameters.feature_count);
        if expected == 0 {
            return Err(PredictionError::NotFitted);
        }
        if expected > MAX_FEATURES {
            return Err(PredictionError::InvalidParameters {
                feature_count: parameters.feature_count,
            });
        }
        if input.features.len() != expected {
            return Err(PredictionError::FeatureCount {
                expected,
                actual: input.features.len(),
            });
        }

        let mut output = parameters.intercept;
        for (index, feature) in input.features.iter().copied().enumerate() {
            output[0] += parameters.coefficients[0][index] * feature;
            output[1] += parameters.coefficients[1][index] * feature;
        }
        Ok(GazePoint::new(output[0], output[1]))
    }

    fn is_fitted(&self) -> bool {
        let count = usize::from(self.parameters.feature_count);
        self.parameters.version == MODEL_FORMAT_VERSION && count > 0 && count <= MAX_FEATURES
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FeatureVector, GazeInput, GazePredictor, LinearPredictor, ModelParameters, PredictionError,
        MODEL_FORMAT_VERSION, OCTA_LAYOUT, QUAD_LAYOUT,
    };

    #[test]
    fn the_quad_layout_is_four_named_features() {
        assert_eq!(QUAD_LAYOUT.len(), 4);
        assert_eq!(QUAD_LAYOUT.index_of("nw"), Some(0));
        assert_eq!(QUAD_LAYOUT.index_of("se"), Some(3));
        assert_eq!(QUAD_LAYOUT.index_of("missing"), None);
    }

    #[test]
    fn the_octa_layout_is_eight_named_features() {
        assert_eq!(OCTA_LAYOUT.len(), 8);
        assert_eq!(OCTA_LAYOUT.labels[0], "left_nw");
        assert_eq!(OCTA_LAYOUT.labels[7], "right_se");
    }

    #[test]
    fn a_feature_vector_keeps_order_and_rejects_overflow() {
        let vector = FeatureVector::new(&[1.0, 2.0, 3.0]).unwrap();

        assert_eq!(vector.len(), 3);
        assert_eq!(vector.as_slice(), &[1.0, 2.0, 3.0]);
        assert_eq!(vector.get(2), Some(3.0));
        assert_eq!(vector.get(3), None);
        assert!(FeatureVector::new(&[0.0; 17]).is_none());
    }

    #[test]
    fn a_quad_vector_is_four_features() {
        let vector = FeatureVector::from_quad([10.0, 20.0, 30.0, 40.0]);

        assert_eq!(vector.len(), 4);
        assert_eq!(vector.as_slice(), &[10.0, 20.0, 30.0, 40.0]);
    }

    #[test]
    fn a_linear_predictor_computes_the_plane() {
        let mut coefficients = [[0.0; super::MAX_FEATURES]; 2];
        coefficients[0][0] = 1.0;
        coefficients[0][1] = 2.0;
        coefficients[1][0] = 3.0;
        coefficients[1][1] = 4.0;
        let parameters = ModelParameters::new(2, coefficients, [5.0, 6.0]).unwrap();
        let predictor = LinearPredictor::new(parameters);

        let point = predictor.predict(GazeInput::glance(&[1.0, 2.0])).unwrap();

        assert_eq!(point.x, 10.0);
        assert_eq!(point.y, 17.0);
        assert!(predictor.is_fitted());
    }

    #[test]
    fn a_predictor_rejects_the_wrong_feature_count_or_an_unfitted_block() {
        let coefficients = [[0.0; super::MAX_FEATURES]; 2];
        let predictor =
            LinearPredictor::new(ModelParameters::new(4, coefficients, [0.0; 2]).unwrap());

        assert_eq!(
            predictor.predict(GazeInput::glance(&[1.0])),
            Err(PredictionError::FeatureCount {
                expected: 4,
                actual: 1,
            })
        );
        assert_eq!(
            LinearPredictor::new(ModelParameters::default()).predict(GazeInput::glance(&[])),
            Err(PredictionError::NotFitted)
        );

        let corrupt = ModelParameters {
            version: MODEL_FORMAT_VERSION,
            feature_count: 17,
            ..ModelParameters::default()
        };
        assert_eq!(
            LinearPredictor::new(corrupt).predict(GazeInput::glance(&[0.0; 17])),
            Err(PredictionError::InvalidParameters { feature_count: 17 })
        );
        assert!(!LinearPredictor::new(corrupt).is_fitted());
    }
}

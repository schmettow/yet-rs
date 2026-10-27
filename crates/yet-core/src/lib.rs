// SPDX-License-Identifier: GPL-3.0-only

//! Abstract gaze-tracking essentials for yet-rs.
//!
//! This crate is `no_std` (with `alloc`) and knows nothing about windows,
//! image decoding, files or audio. It defines the vocabulary that a tracker,
//! a stimulus presenter and a microcontroller implementation share:
//!
//! * monotonic [`SessionTime`] and the [`Clock`] seam,
//! * camera roles and the borrowed [`FrameSet`] handed to feature extractors,
//! * grayscale frames, eye regions and the Quadbright [`FeatureExtractor`],
//! * the portable [`GazePredictor`] contract and [`ModelParameters`] format.
//!
//! The reference Quadbright compressor and the host-side linear trainer are
//! compiled with the non-default `yet-zero` feature. A `no_std` consumer that
//! only deploys a pre-trained model needs neither.

#![no_std]

extern crate alloc;

pub mod camera;
pub mod clock;
pub mod gaze;
pub mod time;
pub mod tracking;

pub use camera::{CameraFrame, CameraRole, FrameSet};
pub use clock::{Clock, TestClock};
pub use gaze::{
    BodyPose, FeatureLayout, FeatureVector, GazeInput, GazePoint, GazePredictor, HeadPose,
    LinearPredictor, ModelParameters, PoseSample, PredictionError, MAX_FEATURES,
    MODEL_FORMAT_VERSION, OCTA_LAYOUT, QUAD_LAYOUT,
};
pub use time::SessionTime;
pub use tracking::{
    quad_brightness, rec601_luma, validate_eye_region, CalibrationObservation, CalibrationSample,
    EyeRegion, FeatureExtractor, FrameError, GrayFrame, QuadBrightness, QuadExtractor,
};

#[cfg(feature = "yet-zero")]
pub use tracking::{
    minimum_samples, Calibration, CalibrationBuffer, CalibrationReport, GazeModel, GazeTrainer,
    LinearGazeModel, LinearParameters, ModelError, MIN_CALIBRATION_SAMPLES,
};

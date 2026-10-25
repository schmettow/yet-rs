// SPDX-License-Identifier: GPL-3.0-only

use yet_core::{Calibration, GazePoint, ModelError, QuadBrightness};

fn features() -> Vec<QuadBrightness> {
    vec![
        QuadBrightness::new(200.0, 128.0, 128.0, 128.0),
        QuadBrightness::new(128.0, 200.0, 128.0, 128.0),
        QuadBrightness::new(128.0, 128.0, 200.0, 128.0),
        QuadBrightness::new(128.0, 128.0, 128.0, 200.0),
        QuadBrightness::new(100.0, 100.0, 100.0, 100.0),
        QuadBrightness::new(150.0, 110.0, 190.0, 130.0),
        QuadBrightness::new(110.0, 190.0, 130.0, 150.0),
        QuadBrightness::new(190.0, 130.0, 150.0, 110.0),
        QuadBrightness::new(130.0, 150.0, 110.0, 190.0),
    ]
}

fn truth(features: QuadBrightness) -> GazePoint {
    GazePoint::new(
        100.0 + 3.0 * features.nw - 2.0 * features.ne + 1.5 * features.sw - 0.5 * features.se,
        50.0 - features.nw + 2.5 * features.ne + 0.75 * features.sw - 1.25 * features.se,
    )
}

fn calibrated() -> Calibration {
    let mut calibration = Calibration::new();
    for features in features() {
        calibration.record(truth(features), features);
    }
    calibration
}

#[test]
fn a_full_calibration_fits_and_reports_rmse() {
    let mut calibration = calibrated();
    assert!(!calibration.is_fitted());

    let report = calibration.fit().unwrap();

    assert!(calibration.is_fitted());
    assert_eq!(report.samples, 9);
    assert!(report.rmse < 0.01);
    assert_eq!(calibration.buffer().len(), 9);
    assert!(calibration.parameters().is_some());
}

#[test]
fn quick_recalibration_adds_samples_and_refits() {
    let mut calibration = calibrated();
    let first = calibration.fit().unwrap();

    // A quick center recalibration adds one more observation.
    let center = QuadBrightness::new(128.0, 128.0, 128.0, 128.0);
    calibration.record(truth(center), center);
    let second = calibration.fit().unwrap();

    assert_eq!(first.samples, 9);
    assert_eq!(second.samples, 10);
    assert!(second.rmse < 0.01);
    assert_eq!(calibration.buffer().len(), 10);

    let predicted = calibration.predict(center).unwrap();
    assert!((predicted.x - truth(center).x).abs() < 0.01);
}

#[test]
fn too_few_samples_are_reported_without_a_fit() {
    let mut calibration = Calibration::new();
    calibration.record(GazePoint::new(10.0, 20.0), QuadBrightness::default());

    let error = calibration.fit().unwrap_err();

    assert_eq!(
        error,
        ModelError::NotEnoughSamples {
            required: 4,
            actual: 1,
        }
    );
    assert!(!calibration.is_fitted());
    assert_eq!(
        calibration.predict(QuadBrightness::default()),
        Err(ModelError::NotFitted)
    );
}

#[test]
fn reset_clears_samples_and_model() {
    let mut calibration = calibrated();
    calibration.fit().unwrap();

    calibration.reset();

    assert!(!calibration.is_fitted());
    assert!(calibration.buffer().is_empty());
    assert_eq!(calibration.parameters(), None);
}

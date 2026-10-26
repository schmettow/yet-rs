// SPDX-License-Identifier: GPL-3.0-only

use yet_core::{
    minimum_samples, CalibrationBuffer, CalibrationSample, FeatureVector, GazeInput, GazePoint,
    GazePredictor, GazeTrainer, LinearGazeModel, LinearPredictor, ModelError, PredictionError,
    QuadBrightness,
};

/// Features spread over all four quadrants, so the 4-column design matrix has
/// full rank. Values are in the 0..=255 brightness range.
fn training_features() -> Vec<QuadBrightness> {
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

fn truth_b(features: QuadBrightness) -> GazePoint {
    GazePoint::new(
        20.0 + 0.4 * features.nw + 0.3 * features.ne + 0.2 * features.sw + 0.1 * features.se,
        10.0 - 0.2 * features.nw + 0.5 * features.ne - 0.1 * features.sw + 0.3 * features.se,
    )
}

fn samples() -> Vec<CalibrationSample> {
    training_features()
        .into_iter()
        .map(|features| CalibrationSample::new(features, truth(features)))
        .collect()
}

#[test]
fn quadrant_values_round_trip_through_the_canonical_array() {
    let features = QuadBrightness::new(1.0, 2.0, 3.0, 4.0);

    assert_eq!(features.to_array(), [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(QuadBrightness::from_array([1.0, 2.0, 3.0, 4.0]), features);
}

#[test]
fn an_unfitted_model_cannot_predict() {
    let model = LinearGazeModel::new();
    let features = QuadBrightness::new(1.0, 2.0, 3.0, 4.0);

    assert!(!model.is_fitted());
    assert_eq!(model.predict(features), Err(ModelError::NotFitted));
}

#[test]
fn a_linear_map_is_recovered_on_held_out_features() {
    let mut model = LinearGazeModel::new();

    model.fit(&samples()).unwrap();

    assert!(model.is_fitted());
    for features in [
        QuadBrightness::new(170.0, 140.0, 120.0, 160.0),
        QuadBrightness::new(120.0, 160.0, 180.0, 140.0),
        QuadBrightness::new(160.0, 120.0, 160.0, 120.0),
    ] {
        let predicted = model.predict(features).unwrap();
        let expected = truth(features);
        assert!(
            (predicted.x - expected.x).abs() < 0.01,
            "x: {predicted:?} vs {expected:?}"
        );
        assert!(
            (predicted.y - expected.y).abs() < 0.01,
            "y: {predicted:?} vs {expected:?}"
        );
    }
}

#[test]
fn refitting_replaces_the_previous_map() {
    let features = QuadBrightness::new(170.0, 140.0, 120.0, 160.0);
    let mut model = LinearGazeModel::new();

    model.fit(&samples()).unwrap();
    let first = model.predict(features).unwrap();

    let second_samples: Vec<CalibrationSample> = training_features()
        .into_iter()
        .map(|features| CalibrationSample::new(features, truth_b(features)))
        .collect();
    model.fit(&second_samples).unwrap();
    let second = model.predict(features).unwrap();

    assert!((first.x - truth(features).x).abs() < 0.01);
    assert!((second.x - truth_b(features).x).abs() < 0.01);
    assert!((second.y - truth_b(features).y).abs() < 0.01);
}

#[test]
fn fewer_than_four_samples_are_rejected() {
    let mut model = LinearGazeModel::new();

    let error = model.fit(&samples()[..3]).unwrap_err();

    assert_eq!(
        error,
        ModelError::NotEnoughSamples {
            required: 4,
            actual: 3,
        }
    );
    assert!(!model.is_fitted());
}

#[test]
fn reset_clears_the_fit() {
    let mut model = LinearGazeModel::new();
    model.fit(&samples()).unwrap();

    model.reset();

    assert!(!model.is_fitted());
    assert_eq!(
        model.predict(QuadBrightness::new(1.0, 2.0, 3.0, 4.0)),
        Err(ModelError::NotFitted)
    );
}

#[test]
fn a_degenerate_calibration_predicts_the_mean_target() {
    let features = QuadBrightness::new(128.0, 128.0, 128.0, 128.0);
    let samples: Vec<CalibrationSample> = [(0.0, 0.0), (10.0, 20.0), (20.0, 40.0), (30.0, 60.0)]
        .into_iter()
        .map(|(x, y)| CalibrationSample::new(features, GazePoint::new(x, y)))
        .collect();
    let mut model = LinearGazeModel::new();

    model.fit(&samples).unwrap();
    let predicted = model.predict(features).unwrap();

    assert!(predicted.x.is_finite() && predicted.y.is_finite());
    assert!((predicted.x - 15.0).abs() < 1e-3, "{predicted:?}");
    assert!((predicted.y - 30.0).abs() < 1e-3, "{predicted:?}");
}

#[test]
fn a_calibration_buffer_fits_the_model_and_reports_rmse() {
    let mut buffer = CalibrationBuffer::new();
    for sample in samples() {
        buffer.push(sample);
    }
    let mut model = LinearGazeModel::new();

    buffer.fit(&mut model).unwrap();
    let rmse = buffer.rmse(&model).unwrap().unwrap();

    assert_eq!(buffer.len(), 9);
    assert!(rmse < 0.01, "rmse: {rmse}");
}

#[test]
fn an_empty_buffer_has_no_rmse() {
    let buffer = CalibrationBuffer::new();
    let model = LinearGazeModel::new();

    assert_eq!(buffer.rmse(&model), Ok(None));
}

#[test]
fn a_buffer_without_a_fitted_model_reports_not_fitted() {
    let mut buffer = CalibrationBuffer::new();
    buffer.push(CalibrationSample::new(
        QuadBrightness::new(1.0, 2.0, 3.0, 4.0),
        GazePoint::new(10.0, 20.0),
    ));

    assert_eq!(
        buffer.rmse(&LinearGazeModel::new()),
        Err(ModelError::NotFitted)
    );
}

#[test]
fn rmse_is_the_euclidean_rms_error() {
    struct Constant(GazePoint);

    impl GazeTrainer for Constant {
        fn fit(&mut self, _samples: &[CalibrationSample]) -> Result<(), ModelError> {
            Ok(())
        }

        fn reset(&mut self) {}
    }

    impl GazePredictor for Constant {
        fn predict(&self, _input: GazeInput<'_>) -> Result<GazePoint, PredictionError> {
            Ok(self.0)
        }

        fn is_fitted(&self) -> bool {
            true
        }
    }

    let mut buffer = CalibrationBuffer::new();
    buffer.push(CalibrationSample::new(
        QuadBrightness::default(),
        GazePoint::new(3.0, 4.0),
    ));

    let rmse = buffer.rmse(&Constant(GazePoint::new(0.0, 0.0))).unwrap();

    assert_eq!(rmse, Some(5.0));
}

#[test]
fn a_thousand_refits_stay_within_a_second() {
    let samples = samples();
    let mut model = LinearGazeModel::new();

    let start = std::time::Instant::now();
    for _ in 0..1000 {
        model.fit(&samples).unwrap();
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "1000 refits took {elapsed:?}"
    );
}

#[test]
fn clearing_the_buffer_removes_its_samples() {
    let mut buffer = CalibrationBuffer::new();
    buffer.push(CalibrationSample::default());

    buffer.clear();

    assert!(buffer.is_empty());
    assert_eq!(buffer.samples(), &[]);
}

#[test]
fn an_eight_feature_model_uses_the_layout_dimension() {
    let mut coefficients = [[0.0_f32; 8]; 2];
    for (index, value) in coefficients[0].iter_mut().enumerate() {
        *value = index as f32 + 1.0;
    }
    for (index, value) in coefficients[1].iter_mut().enumerate() {
        *value = 8.0 - index as f32;
    }
    let intercept = [10.0_f32, 20.0];
    let patterns: [[f32; 8]; 9] = [
        [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0],
    ];
    let samples: Vec<CalibrationSample> = patterns
        .iter()
        .map(|pattern| {
            let x = intercept[0]
                + pattern
                    .iter()
                    .zip(coefficients[0])
                    .map(|(feature, coefficient)| feature * coefficient)
                    .sum::<f32>();
            let y = intercept[1]
                + pattern
                    .iter()
                    .zip(coefficients[1])
                    .map(|(feature, coefficient)| feature * coefficient)
                    .sum::<f32>();
            CalibrationSample::from_features(
                FeatureVector::new(pattern).unwrap(),
                GazePoint::new(x, y),
            )
        })
        .collect();

    assert_eq!(minimum_samples(8), 9);
    let mut model = LinearGazeModel::new();
    model.fit(&samples).unwrap();
    let predicted = GazePredictor::predict(&model, GazeInput::glance(&patterns[1])).unwrap();
    assert!((predicted.x - 12.0).abs() < 0.05, "{}", predicted.x);
    assert!((predicted.y - 27.0).abs() < 0.05, "{}", predicted.y);

    let parameters = model.model_parameters().unwrap();
    assert_eq!(parameters.feature_count, 8);
    let portable = LinearPredictor::new(parameters);
    let portable_point = portable.predict(GazeInput::glance(&patterns[1])).unwrap();
    assert!((portable_point.x - predicted.x).abs() < 0.1);
    assert!((portable_point.y - predicted.y).abs() < 0.1);
}

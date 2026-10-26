// SPDX-License-Identifier: GPL-3.0-only

//! Python-oracle parity fixtures for CV-4.
//!
//! The fixtures are synthetic and tiny; `tools/cv4_fixtures.py` generated them
//! with NumPy, OpenCV and scikit-learn from the yet_zero virtualenv. Adding
//! recorded eye crops means appending raw `.gray` files and rows to
//! `features/features.csv`; no test change is needed.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use yet_core::{
    quad_brightness, rec601_luma, CalibrationSample, EyeRegion, GazePoint, GrayFrame,
    LinearGazeModel, QuadBrightness,
};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cv4")
}

#[derive(Debug, Deserialize)]
struct FeatureRow {
    file: String,
    width: u32,
    height: u32,
    nw: f32,
    ne: f32,
    sw: f32,
    se: f32,
}

#[test]
fn quadrant_means_match_the_python_oracle() {
    let directory = fixtures().join("features");
    let mut reader = csv::Reader::from_path(directory.join("features.csv")).unwrap();
    let rows: Vec<FeatureRow> = reader.deserialize().collect::<Result<_, _>>().unwrap();

    assert_eq!(rows.len(), 4);
    let mut saw_odd_width = false;
    let mut saw_odd_height = false;

    for row in rows {
        saw_odd_width |= row.width % 2 == 1;
        saw_odd_height |= row.height % 2 == 1;

        let data = fs::read(directory.join(&row.file)).unwrap();
        let frame = GrayFrame::new(row.width, row.height, data).unwrap();
        let found = quad_brightness(&frame, EyeRegion::new(0, 0, row.width, row.height))
            .expect("fixture must be at least 2x2");
        let expected = [row.nw, row.ne, row.sw, row.se];

        for (index, (found, expected)) in found.to_array().iter().zip(expected).enumerate() {
            assert!(
                (found - expected).abs() < 1e-3,
                "{} quadrant {index}: {found} vs {expected}",
                row.file
            );
        }
    }

    assert!(saw_odd_width, "fixtures must include an odd width");
    assert!(saw_odd_height, "fixtures must include an odd height");
}

#[derive(Debug, Deserialize)]
struct CalibrationRow {
    nw: f32,
    ne: f32,
    sw: f32,
    se: f32,
    target_x: f32,
    target_y: f32,
}

#[derive(Debug, Deserialize)]
struct PredictionRow {
    nw: f32,
    ne: f32,
    sw: f32,
    se: f32,
    predicted_x: f32,
    predicted_y: f32,
}

#[derive(Debug, Deserialize)]
struct CoefficientRow {
    output: String,
    intercept: f64,
    nw: f64,
    ne: f64,
    sw: f64,
    se: f64,
}

#[test]
fn the_least_squares_fit_matches_sklearn() {
    let directory = fixtures().join("regression");
    let calibration: Vec<CalibrationRow> =
        csv::Reader::from_path(directory.join("calibration.csv"))
            .unwrap()
            .deserialize()
            .collect::<Result<_, _>>()
            .unwrap();
    assert_eq!(calibration.len(), 12);

    let samples: Vec<CalibrationSample> = calibration
        .iter()
        .take(9)
        .map(|row| {
            CalibrationSample::new(
                QuadBrightness::new(row.nw, row.ne, row.sw, row.se),
                GazePoint::new(row.target_x, row.target_y),
            )
        })
        .collect();
    let mut model = LinearGazeModel::new();
    model.fit(&samples).unwrap();

    let predictions: Vec<PredictionRow> = csv::Reader::from_path(directory.join("predictions.csv"))
        .unwrap()
        .deserialize()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(predictions.len(), 3);

    for row in &predictions {
        let features = QuadBrightness::new(row.nw, row.ne, row.sw, row.se);
        let predicted = model.predict(features).unwrap();

        assert!(
            (predicted.x - row.predicted_x).abs() < 0.5,
            "x: {} vs {}",
            predicted.x,
            row.predicted_x
        );
        assert!(
            (predicted.y - row.predicted_y).abs() < 0.5,
            "y: {} vs {}",
            predicted.y,
            row.predicted_y
        );
    }

    let parameters = model.parameters().unwrap();
    let coefficients: Vec<CoefficientRow> =
        csv::Reader::from_path(directory.join("coefficients.csv"))
            .unwrap()
            .deserialize()
            .collect::<Result<_, _>>()
            .unwrap();
    assert_eq!(coefficients.len(), 2);

    for (axis, expected) in coefficients.iter().enumerate() {
        assert_eq!(expected.output, if axis == 0 { "x" } else { "y" });
        assert!(
            (parameters.intercept[axis] - expected.intercept).abs() < 1e-6,
            "intercept {axis}: {} vs {}",
            parameters.intercept[axis],
            expected.intercept
        );
        for (index, expected) in [expected.nw, expected.ne, expected.sw, expected.se]
            .into_iter()
            .enumerate()
        {
            assert!(
                (parameters.coefficients[axis][index] - expected).abs() < 1e-6,
                "coefficient {axis}/{index}: {} vs {expected}",
                parameters.coefficients[axis][index]
            );
        }
    }
}

#[derive(Debug, Deserialize)]
struct LumaMeta {
    rgb: String,
    gray: String,
    width: u32,
    height: u32,
}

#[test]
fn rec601_luma_matches_opencv_on_an_odd_fixture() {
    let directory = fixtures().join("luma");
    let meta: LumaMeta = csv::Reader::from_path(directory.join("meta.csv"))
        .unwrap()
        .deserialize()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!((meta.width, meta.height), (7, 5));

    let rgb = fs::read(directory.join(&meta.rgb)).unwrap();
    let gray = fs::read(directory.join(&meta.gray)).unwrap();
    assert_eq!(rgb.len(), meta.width as usize * meta.height as usize * 3);
    assert_eq!(gray.len(), meta.width as usize * meta.height as usize);

    for (index, expected) in gray.iter().enumerate() {
        let offset = index * 3;
        let found = rec601_luma(rgb[offset], rgb[offset + 1], rgb[offset + 2]);
        assert_eq!(found, *expected, "pixel {index}");
    }
}

#[test]
fn opencv_fixed_point_can_differ_by_one_on_arbitrary_colors() {
    // OpenCV 4.11 BGR2GRAY returns 28 for RGB (0, 0, 250); the documented
    // rounding formula returns 29. Camera-frame parity must therefore allow a
    // one-level difference, while the committed fixture happens to match
    // exactly.
    let opencv = 28;
    let found = i32::from(rec601_luma(0, 0, 250));

    assert!(
        (found - opencv).abs() <= 1,
        "found {found}, OpenCV {opencv}"
    );
}

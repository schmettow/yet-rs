// SPDX-License-Identifier: GPL-3.0-only

use yet_core::{quad_brightness, rec601_luma, EyeRegion, FrameError, GrayFrame, QuadBrightness};

fn frame(width: u32, height: u32, data: Vec<u8>) -> GrayFrame {
    GrayFrame::new(width, height, data).unwrap()
}

#[test]
fn a_frame_rejects_a_buffer_with_the_wrong_length() {
    assert_eq!(
        GrayFrame::new(2, 2, vec![0; 3]).unwrap_err(),
        FrameError::InvalidLength {
            width: 2,
            height: 2,
            expected: 4,
            actual: 3,
        }
    );
    assert_eq!(
        GrayFrame::new(0, 2, Vec::new()).unwrap_err(),
        FrameError::Empty {
            width: 0,
            height: 2,
        }
    );
}

#[test]
fn pixels_are_addressed_row_major() {
    let frame = frame(3, 2, vec![1, 2, 3, 4, 5, 6]);

    assert_eq!(frame.width(), 3);
    assert_eq!(frame.height(), 2);
    assert_eq!(frame.data(), &[1, 2, 3, 4, 5, 6]);
    assert_eq!(frame.pixel(0, 0), Some(1));
    assert_eq!(frame.pixel(2, 0), Some(3));
    assert_eq!(frame.pixel(0, 1), Some(4));
    assert_eq!(frame.pixel(2, 1), Some(6));
    assert_eq!(frame.pixel(3, 1), None);
}

#[test]
fn quadrant_means_use_the_canonical_north_west_to_south_east_order() {
    // Four 2x2 quadrants: NW=10, NE=20, SW=30, SE=40.
    let data = vec![
        10, 10, 20, 20, //
        10, 10, 20, 20, //
        30, 30, 40, 40, //
        30, 30, 40, 40,
    ];
    let frame = frame(4, 4, data);

    assert_eq!(
        quad_brightness(&frame, EyeRegion::new(0, 0, 4, 4)),
        Some(QuadBrightness::new(10.0, 20.0, 30.0, 40.0))
    );
}

#[test]
fn the_remainder_pixel_goes_to_the_east_and_south_quadrants() {
    // A 5x5 frame whose pixel values are their row-major index.
    let frame = frame(5, 5, (0..25).collect());

    let means = quad_brightness(&frame, EyeRegion::new(0, 0, 5, 5)).unwrap();

    assert_eq!(means, QuadBrightness::new(3.0, 5.5, 15.5, 18.0));
}

#[test]
fn an_eye_region_is_cropped_before_the_split() {
    // 6x6 frame where each pixel value is row * 10 + column.
    let data: Vec<u8> = (0..6)
        .flat_map(|row| (0..6).map(move |column| (row * 10 + column) as u8))
        .collect();
    let frame = frame(6, 6, data);

    // Region x=2..6, y=1..5: mid_x at column 4, mid_y at row 3.
    let means = quad_brightness(&frame, EyeRegion::new(2, 1, 4, 4)).unwrap();

    assert_eq!(means, QuadBrightness::new(17.5, 19.5, 37.5, 39.5));
}

#[test]
fn regions_that_cannot_be_split_have_no_features() {
    let frame = frame(4, 4, vec![0; 16]);

    assert_eq!(quad_brightness(&frame, EyeRegion::new(0, 0, 1, 4)), None);
    assert_eq!(quad_brightness(&frame, EyeRegion::new(0, 0, 4, 1)), None);
    assert_eq!(quad_brightness(&frame, EyeRegion::new(0, 0, 0, 4)), None);
    assert_eq!(quad_brightness(&frame, EyeRegion::new(3, 3, 2, 2)), None);
    assert_eq!(quad_brightness(&frame, EyeRegion::new(0, 0, 5, 4)), None);
    assert_eq!(quad_brightness(&frame, EyeRegion::new(0, 0, 4, 5)), None);
}

#[test]
fn a_region_inside_the_frame_is_accepted_at_the_last_pixel() {
    let frame = frame(4, 4, vec![1; 16]);

    assert_eq!(
        quad_brightness(&frame, EyeRegion::new(2, 2, 2, 2)),
        Some(QuadBrightness::new(1.0, 1.0, 1.0, 1.0))
    );
}

#[test]
fn rec601_luma_matches_opencv_bgr2gray() {
    // SPK-3 measured OpenCV BGR2GRAY: red -> 76, green -> 150, blue -> 29.
    assert_eq!(rec601_luma(255, 0, 0), 76);
    assert_eq!(rec601_luma(0, 255, 0), 150);
    assert_eq!(rec601_luma(0, 0, 255), 29);
    assert_eq!(rec601_luma(0, 0, 0), 0);
    assert_eq!(rec601_luma(255, 255, 255), 255);
}

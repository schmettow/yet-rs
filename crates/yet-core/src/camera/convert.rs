// SPDX-License-Identifier: GPL-3.0-only

//! Conversion from camera buffers to [`GrayFrame`].
//!
//! The conversions are pure and unit-tested; platform adapters only choose a
//! format and pass the buffer, stride and dimensions.

use crate::tracking::{rec601_luma, GrayFrame};

use super::CameraError;

/// Extract the Y bytes of packed YUYV, honouring the row stride.
pub(crate) fn yuyv_to_gray(
    buffer: &[u8],
    width: u32,
    height: u32,
    stride: u32,
) -> Result<GrayFrame, CameraError> {
    validate_dimensions(width, height)?;
    let stride = stride.max(width.saturating_mul(2));
    let required = required_bytes(width, height, stride, width as usize * 2)?;

    if buffer.len() < required {
        return Err(CameraError::Truncated {
            expected: required,
            actual: buffer.len(),
        });
    }

    let mut data = Vec::with_capacity(pixel_count(width, height));
    for row in 0..height as usize {
        let start = row * stride as usize;
        for column in 0..width as usize {
            data.push(buffer[start + column * 2]);
        }
    }
    gray_frame(width, height, data)
}

/// Copy an 8-bit grayscale buffer row by row, honouring the row stride.
pub(crate) fn gray_to_gray(
    buffer: &[u8],
    width: u32,
    height: u32,
    stride: u32,
) -> Result<GrayFrame, CameraError> {
    validate_dimensions(width, height)?;
    let stride = stride.max(width);
    let required = required_bytes(width, height, stride, width as usize)?;

    if buffer.len() < required {
        return Err(CameraError::Truncated {
            expected: required,
            actual: buffer.len(),
        });
    }

    let mut data = Vec::with_capacity(pixel_count(width, height));
    for row in 0..height as usize {
        let start = row * stride as usize;
        data.extend_from_slice(&buffer[start..start + width as usize]);
    }
    gray_frame(width, height, data)
}

/// Decode an MJPEG frame and convert RGB to Rec. 601 luma.
pub(crate) fn mjpeg_to_gray(buffer: &[u8]) -> Result<GrayFrame, CameraError> {
    let decoded = image::load_from_memory_with_format(buffer, image::ImageFormat::Jpeg)
        .map_err(|error| CameraError::Decode(error.to_string()))?;
    let rgb = decoded.to_rgb8();
    let (width, height) = rgb.dimensions();
    let data = rgb
        .pixels()
        .map(|pixel| rec601_luma(pixel[0], pixel[1], pixel[2]))
        .collect();
    gray_frame(width, height, data)
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), CameraError> {
    if width == 0 || height == 0 {
        Err(CameraError::InvalidDimensions { width, height })
    } else {
        Ok(())
    }
}

fn required_bytes(
    width: u32,
    height: u32,
    stride: u32,
    bytes_per_row: usize,
) -> Result<usize, CameraError> {
    (height as usize - 1)
        .checked_mul(stride as usize)
        .and_then(|last_row| last_row.checked_add(bytes_per_row))
        .ok_or(CameraError::InvalidDimensions { width, height })
}

fn pixel_count(width: u32, height: u32) -> usize {
    (width as usize).saturating_mul(height as usize)
}

fn gray_frame(width: u32, height: u32, data: Vec<u8>) -> Result<GrayFrame, CameraError> {
    GrayFrame::new(width, height, data)
        .map_err(|_| CameraError::InvalidDimensions { width, height })
}

#[cfg(test)]
mod tests {
    use image::codecs::jpeg::JpegEncoder;
    use image::ExtendedColorType;

    use super::*;

    #[test]
    fn yuyv_takes_only_the_luma_bytes_and_honours_the_stride() {
        // 2x2, stride 6 (one padding pair per row). Y values 10, 20, 30, 40.
        let buffer = [
            10, 1, 20, 2, 0, 0, //
            30, 3, 40, 4, 0, 0,
        ];

        let frame = yuyv_to_gray(&buffer, 2, 2, 6).unwrap();

        assert_eq!(frame.width(), 2);
        assert_eq!(frame.height(), 2);
        assert_eq!(frame.data(), &[10, 20, 30, 40]);
    }

    #[test]
    fn gray_copies_rows_and_skips_padding() {
        let buffer = [1, 2, 0, 0, 3, 4, 0, 0];

        let frame = gray_to_gray(&buffer, 2, 2, 4).unwrap();

        assert_eq!(frame.data(), &[1, 2, 3, 4]);
    }

    #[test]
    fn truncated_buffers_are_rejected() {
        assert_eq!(
            yuyv_to_gray(&[0; 5], 2, 2, 4).unwrap_err(),
            CameraError::Truncated {
                expected: 8,
                actual: 5,
            }
        );
        assert_eq!(
            gray_to_gray(&[0; 3], 2, 2, 2).unwrap_err(),
            CameraError::Truncated {
                expected: 4,
                actual: 3,
            }
        );
    }

    #[test]
    fn empty_dimensions_are_rejected() {
        assert_eq!(
            yuyv_to_gray(&[], 0, 2, 0).unwrap_err(),
            CameraError::InvalidDimensions {
                width: 0,
                height: 2,
            }
        );
    }

    #[test]
    fn a_mjpeg_frame_is_decoded_with_rec601_luma() {
        // 16x16 with four solid 8x8 quadrants: red, green, blue, gray.
        let mut rgb = Vec::with_capacity(16 * 16 * 3);
        for y in 0..16 {
            for x in 0..16 {
                let pixel = match (x < 8, y < 8) {
                    (true, true) => [255, 0, 0],
                    (false, true) => [0, 255, 0],
                    (true, false) => [0, 0, 255],
                    (false, false) => [128, 128, 128],
                };
                rgb.extend_from_slice(&pixel);
            }
        }
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 100)
            .encode(&rgb, 16, 16, ExtendedColorType::Rgb8)
            .unwrap();

        let frame = mjpeg_to_gray(&jpeg).unwrap();

        assert_eq!((frame.width(), frame.height()), (16, 16));
        assert!((i32::from(frame.pixel(4, 4).unwrap()) - 76).abs() <= 3);
        assert!((i32::from(frame.pixel(12, 4).unwrap()) - 150).abs() <= 3);
        assert!((i32::from(frame.pixel(4, 12).unwrap()) - 29).abs() <= 3);
        assert!((i32::from(frame.pixel(12, 12).unwrap()) - 128).abs() <= 3);
    }

    #[test]
    fn invalid_mjpeg_data_is_a_decode_error() {
        assert!(matches!(
            mjpeg_to_gray(b"not a jpeg"),
            Err(CameraError::Decode(_))
        ));
    }
}

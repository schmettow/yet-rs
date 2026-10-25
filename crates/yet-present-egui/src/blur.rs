// SPDX-License-Identifier: GPL-3.0-only

//! A separable box blur for cached stimulus previews.
//!
//! The Python prototype blurs the stimulus with `cv.blur` for the quick
//! calibration preview. The preview only needs a visually smooth image, so a
//! simple box filter with duplicated borders is enough and does not need an
//! image-processing crate.
//!
//! This is not a pixel-exact port of OpenCV's default border mode; the preview
//! is a visual aid, not a measured stimulus.

use image::RgbaImage;

/// Blur an RGBA image with a box kernel of `radius` pixels.
///
/// Border pixels are duplicated. Radius `0` returns a copy. The implementation
/// is separable and runs in linear time in the number of pixels, independent of
/// the radius.
#[must_use]
pub fn box_blur_rgba(image: &RgbaImage, radius: u32) -> RgbaImage {
    let (width, height) = image.dimensions();
    if radius == 0 || width == 0 || height == 0 {
        return image.clone();
    }

    let src = image.as_raw();
    let horizontal = blur_rows(src, width, height, radius);
    let blurred = blur_columns(&horizontal, width, height, radius);
    RgbaImage::from_raw(width, height, blurred).expect("blur preserves the buffer size")
}

fn blur_rows(src: &[u8], width: u32, height: u32, radius: u32) -> Vec<u8> {
    let width = width as usize;
    let height = height as usize;
    let radius = radius as usize;
    let window = (radius * 2 + 1) as u32;
    let mut out = vec![0_u8; src.len()];
    let mut prefix = vec![0_u32; (width + 1) * 4];

    for y in 0..height {
        let row = y * width * 4;
        prefix[..4].fill(0);
        for x in 0..width {
            let at = row + x * 4;
            let previous = x * 4;
            let next = (x + 1) * 4;
            for channel in 0..4 {
                prefix[next + channel] = prefix[previous + channel] + u32::from(src[at + channel]);
            }
        }

        for x in 0..width {
            let left = x.saturating_sub(radius);
            let right = (x + radius).min(width - 1);
            let duplicate_left = radius.saturating_sub(x) as u32;
            let duplicate_right = (x + radius).saturating_sub(width - 1) as u32;
            let at = row + x * 4;
            for channel in 0..4 {
                let sum = prefix[(right + 1) * 4 + channel] - prefix[left * 4 + channel]
                    + duplicate_left * u32::from(src[row + channel])
                    + duplicate_right * u32::from(src[row + (width - 1) * 4 + channel]);
                out[at + channel] = (sum / window) as u8;
            }
        }
    }

    out
}

fn blur_columns(src: &[u8], width: u32, height: u32, radius: u32) -> Vec<u8> {
    let width = width as usize;
    let height = height as usize;
    let radius = radius as usize;
    let window = (radius * 2 + 1) as u32;
    let stride = width * 4;
    let mut out = vec![0_u8; src.len()];
    let mut prefix = vec![0_u32; (height + 1) * 4];

    for x in 0..width {
        let column = x * 4;
        prefix[..4].fill(0);
        for y in 0..height {
            let at = y * stride + column;
            let previous = y * 4;
            let next = (y + 1) * 4;
            for channel in 0..4 {
                prefix[next + channel] = prefix[previous + channel] + u32::from(src[at + channel]);
            }
        }

        for y in 0..height {
            let top = y.saturating_sub(radius);
            let bottom = (y + radius).min(height - 1);
            let duplicate_top = radius.saturating_sub(y) as u32;
            let duplicate_bottom = (y + radius).saturating_sub(height - 1) as u32;
            let at = y * stride + column;
            for channel in 0..4 {
                let sum = prefix[(bottom + 1) * 4 + channel] - prefix[top * 4 + channel]
                    + duplicate_top * u32::from(src[column + channel])
                    + duplicate_bottom * u32::from(src[(height - 1) * stride + column + channel]);
                out[at + channel] = (sum / window) as u8;
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::box_blur_rgba;

    fn image(width: u32, height: u32, pixels: &[[u8; 4]]) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            let [r, g, b, a] = pixels[(y * width + x) as usize];
            Rgba([r, g, b, a])
        })
    }

    #[test]
    fn radius_zero_is_the_identity() {
        let source = image(
            2,
            2,
            &[
                [1, 2, 3, 4],
                [5, 6, 7, 8],
                [9, 10, 11, 12],
                [13, 14, 15, 16],
            ],
        );

        assert_eq!(box_blur_rgba(&source, 0), source);
    }

    #[test]
    fn a_box_blur_averages_each_channel_with_clamped_edges() {
        // A single row: [0, 60, 120]. With radius 1 the window is 3 wide and
        // both edges are duplicated, giving [20, 60, 100].
        let source = image(
            3,
            1,
            &[[0, 0, 0, 255], [60, 60, 60, 255], [120, 120, 120, 255]],
        );

        let blurred = box_blur_rgba(&source, 1);

        let values: Vec<u8> = blurred.pixels().map(|pixel| pixel[0]).collect();
        assert_eq!(values, vec![20, 60, 100]);
        assert!(blurred.pixels().all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn a_constant_image_survives_a_blur() {
        let source = image(4, 3, &[[77, 88, 99, 200]; 12]);

        let blurred = box_blur_rgba(&source, 7);

        assert_eq!(blurred, source);
    }

    #[test]
    fn the_blur_radius_does_not_change_the_size() {
        let source = image(5, 2, &[[10, 20, 30, 40]; 10]);

        assert_eq!(box_blur_rgba(&source, 3).dimensions(), (5, 2));
    }
}

// SPDX-License-Identifier: GPL-3.0-only

//! Grayscale frames and eye regions.
//!
//! A [`GrayFrame`] is a tightly packed, row-major buffer with no stride
//! padding. Camera adapters convert their native format at the boundary; the
//! tracker never sees YUYV, MJPEG or an `image` buffer.

use alloc::vec::Vec;
use thiserror::Error;

/// Errors from constructing a [`GrayFrame`].
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum FrameError {
    /// One of the dimensions is zero.
    #[error("frame dimensions {width}x{height} are empty")]
    Empty {
        /// Frame width.
        width: u32,
        /// Frame height.
        height: u32,
    },
    /// The buffer length does not match `width * height`.
    #[error("frame buffer has {actual} bytes, expected {expected} for {width}x{height}")]
    InvalidLength {
        /// Frame width.
        width: u32,
        /// Frame height.
        height: u32,
        /// Required number of bytes.
        expected: usize,
        /// Supplied number of bytes.
        actual: usize,
    },
    /// `width * height` does not fit in `usize`.
    #[error("frame dimensions {width}x{height} are too large")]
    TooLarge {
        /// Frame width.
        width: u32,
        /// Frame height.
        height: u32,
    },
}

/// A tightly packed 8-bit grayscale frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrayFrame {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl GrayFrame {
    /// Wrap a row-major buffer, checking its length.
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Result<Self, FrameError> {
        if width == 0 || height == 0 {
            return Err(FrameError::Empty { width, height });
        }
        let expected = (width as usize)
            .checked_mul(height as usize)
            .ok_or(FrameError::TooLarge { width, height })?;
        if data.len() != expected {
            return Err(FrameError::InvalidLength {
                width,
                height,
                expected,
                actual: data.len(),
            });
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    /// Frame width in pixels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Frame height in pixels.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The row-major pixel buffer.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// A pixel value, or `None` when the coordinates are outside the frame.
    pub fn pixel(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = y as usize * self.width as usize + x as usize;
        self.data.get(index).copied()
    }

    /// Copy a sub-rectangle into a new tightly packed frame.
    ///
    /// Returns `None` when the region is empty or outside the frame.
    pub fn crop(&self, region: EyeRegion) -> Option<Self> {
        if !region.is_inside(self) {
            return None;
        }

        let mut data = Vec::with_capacity(region.width as usize * region.height as usize);
        for row in region.y..region.y + region.height {
            let start = row as usize * self.width as usize + region.x as usize;
            data.extend_from_slice(&self.data[start..start + region.width as usize]);
        }

        Some(Self {
            width: region.width,
            height: region.height,
            data,
        })
    }
}

/// Whether a normalized eye region is finite and inside the unit square.
pub fn validate_eye_region(normalized: [f32; 4]) -> bool {
    let [x, y, width, height] = normalized;
    [x, y, width, height].iter().all(|value| value.is_finite())
        && x >= 0.0
        && y >= 0.0
        && width > 0.0
        && height > 0.0
        && x + width <= 1.0
        && y + height <= 1.0
}

/// A rectangular eye region in frame pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EyeRegion {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl EyeRegion {
    /// Create an eye region.
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Whether the region has a non-zero size and lies fully inside `frame`.
    pub fn is_inside(&self, frame: &GrayFrame) -> bool {
        if self.width == 0 || self.height == 0 {
            return false;
        }
        let Some(right) = self.x.checked_add(self.width) else {
            return false;
        };
        let Some(bottom) = self.y.checked_add(self.height) else {
            return false;
        };
        right <= frame.width && bottom <= frame.height
    }
}

/// Rec. 601 luma with round-half-up, matching OpenCV `BGR2GRAY`.
///
/// `image`'s `to_luma8` uses Rec. 709 and produces different values; do not use
/// it for camera frames that must match the Python prototype.
pub const fn rec601_luma(r: u8, g: u8, b: u8) -> u8 {
    ((299 * r as u32 + 587 * g as u32 + 114 * b as u32 + 500) / 1000) as u8
}

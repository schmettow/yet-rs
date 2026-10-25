// SPDX-License-Identifier: GPL-3.0-only

//! Quadrant brightness features.

use super::{EyeRegion, GrayFrame};

/// Mean brightness of the four quadrants of an eye region.
///
/// The canonical order is `(nw, ne, sw, se)`. This differs from the Python
/// prototype's CSV column order, which is `(nw, sw, ne, se)`; conversion belongs
/// at the data-import boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuadBrightness {
    /// North-west (top-left) mean.
    pub nw: f32,
    /// North-east (top-right) mean.
    pub ne: f32,
    /// South-west (bottom-left) mean.
    pub sw: f32,
    /// South-east (bottom-right) mean.
    pub se: f32,
}

impl QuadBrightness {
    /// Create quadrant means in canonical order.
    pub const fn new(nw: f32, ne: f32, sw: f32, se: f32) -> Self {
        Self { nw, ne, sw, se }
    }

    /// The means in canonical order.
    pub const fn to_array(self) -> [f32; 4] {
        [self.nw, self.ne, self.sw, self.se]
    }

    /// Build from means in canonical order.
    pub const fn from_array(values: [f32; 4]) -> Self {
        Self::new(values[0], values[1], values[2], values[3])
    }
}

/// Mean brightness of the four quadrants of `region` in `frame`.
///
/// The split is at `width / 2` and `height / 2`; the remainder pixel of an odd
/// dimension belongs to the east or south quadrants. Returns `None` when the
/// region is outside the frame or smaller than 2x2, because at least one
/// quadrant would be empty.
///
/// The means are absolute brightness values. They are deliberately not
/// normalized by the ROI mean or a reference frame; the linear model absorbs
/// the illumination profile.
pub fn quad_brightness(frame: &GrayFrame, region: EyeRegion) -> Option<QuadBrightness> {
    if region.width < 2 || region.height < 2 || !region.is_inside(frame) {
        return None;
    }

    let mid_x = region.width / 2;
    let mid_y = region.height / 2;
    let frame_width = frame.width() as usize;
    let data = frame.data();

    let mut nw = (0_u64, 0_u64);
    let mut ne = (0_u64, 0_u64);
    let mut sw = (0_u64, 0_u64);
    let mut se = (0_u64, 0_u64);

    for row in 0..region.height {
        let row_offset = (region.y + row) as usize * frame_width + region.x as usize;
        for column in 0..region.width {
            let value = data[row_offset + column as usize] as u64;
            let bucket = match (column < mid_x, row < mid_y) {
                (true, true) => &mut nw,
                (false, true) => &mut ne,
                (true, false) => &mut sw,
                (false, false) => &mut se,
            };
            bucket.0 += value;
            bucket.1 += 1;
        }
    }

    Some(QuadBrightness::new(mean(nw), mean(ne), mean(sw), mean(se)))
}

fn mean((sum, count): (u64, u64)) -> f32 {
    debug_assert!(count > 0, "a quadrant must contain at least one pixel");
    sum as f32 / count as f32
}

// SPDX-License-Identifier: GPL-3.0-only

//! Minimal, dependency-free grayscale and quadrant helpers for SPK-3.

#[derive(Clone, Copy, Debug)]
pub struct EyeRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct QuadBrightness {
    pub nw: f32,
    pub ne: f32,
    pub sw: f32,
    pub se: f32,
}

pub struct GrayFrame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Compute the four quadrant means with the canonical `(nw, ne, sw, se)` order.
///
/// The remainder pixel of an odd width/height goes to the east/south quadrant.
pub fn quad_brightness(frame: &GrayFrame, roi: EyeRegion) -> QuadBrightness {
    let mid_x = roi.width / 2;
    let mid_y = roi.height / 2;
    let mut sums = [0_u64; 4];
    let mut counts = [0_u64; 4];

    for y in 0..roi.height {
        for x in 0..roi.width {
            let px = roi.x + x;
            let py = roi.y + y;
            let value = u64::from(frame.data[(py * frame.width + px) as usize]);
            let quadrant = match (x < mid_x, y < mid_y) {
                (true, true) => 0,
                (false, true) => 1,
                (true, false) => 2,
                (false, false) => 3,
            };
            sums[quadrant] += value;
            counts[quadrant] += 1;
        }
    }

    let mean = |index: usize| {
        if counts[index] == 0 {
            0.0
        } else {
            sums[index] as f32 / counts[index] as f32
        }
    };

    QuadBrightness {
        nw: mean(0),
        ne: mean(1),
        sw: mean(2),
        se: mean(3),
    }
}

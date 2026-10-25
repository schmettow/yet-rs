// SPDX-License-Identifier: GPL-3.0-only

//! Physical-pixel geometry shared by the engine and presenters.
//!
//! The canonical coordinate space is monitor-local physical pixels with the
//! origin at the top-left of the target monitor. Logical points exist only at
//! the boundary to a UI toolkit.

/// A point in monitor-local physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointPx {
    /// Horizontal position in physical pixels.
    pub x: f32,
    /// Vertical position in physical pixels.
    pub y: f32,
}

impl PointPx {
    /// Create a physical-pixel point.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A size in physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SizePx {
    /// Width in physical pixels.
    pub width: f32,
    /// Height in physical pixels.
    pub height: f32,
}

impl SizePx {
    /// Create a physical-pixel size.
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// A rectangle in monitor-local physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectPx {
    /// Left edge in physical pixels.
    pub x: f32,
    /// Top edge in physical pixels.
    pub y: f32,
    /// Width in physical pixels.
    pub width: f32,
    /// Height in physical pixels.
    pub height: f32,
}

impl RectPx {
    /// Create a physical-pixel rectangle.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// A rectangle at the origin with the given size.
    pub const fn from_size(size: SizePx) -> Self {
        Self::new(0.0, 0.0, size.width, size.height)
    }

    /// The center point of the rectangle.
    pub fn center(self) -> PointPx {
        PointPx::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// Geometry of the monitor targeted by a presenter or overlay.
///
/// `origin_px` is the monitor's position in the desktop coordinate space. It is
/// `None` on Wayland, where the compositor does not expose global window
/// positions; in that case all coordinates remain monitor-local.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenGeometry {
    /// Zero-based monitor index as reported by the platform.
    pub monitor_index: usize,
    /// Desktop-space origin of the monitor, when known.
    pub origin_px: Option<(i32, i32)>,
    /// Monitor size in physical pixels.
    pub size_px: (u32, u32),
    /// Physical pixels per logical point.
    pub scale_factor: f32,
}

impl ScreenGeometry {
    /// Create monitor geometry.
    ///
    /// `scale_factor` must be positive; it is validated when configuration is
    /// loaded, because it comes from the platform rather than from the engine.
    pub const fn new(
        monitor_index: usize,
        origin_px: Option<(i32, i32)>,
        size_px: (u32, u32),
        scale_factor: f32,
    ) -> Self {
        Self {
            monitor_index,
            origin_px,
            size_px,
            scale_factor,
        }
    }

    /// Convert logical points to physical pixels.
    pub fn points_to_px(&self, points: f32) -> f32 {
        debug_assert!(self.scale_factor > 0.0, "scale factor must be positive");
        points * self.scale_factor
    }

    /// Convert physical pixels to logical points.
    pub fn px_to_points(&self, px: f32) -> f32 {
        debug_assert!(self.scale_factor > 0.0, "scale factor must be positive");
        px / self.scale_factor
    }

    /// Monitor width in physical pixels.
    pub const fn width_px(&self) -> u32 {
        self.size_px.0
    }

    /// Monitor height in physical pixels.
    pub const fn height_px(&self) -> u32 {
        self.size_px.1
    }
}

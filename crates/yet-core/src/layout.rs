// SPDX-License-Identifier: GPL-3.0-only

//! Stimulus placement and text scenes in monitor-local physical pixels.
//!
//! The Python prototype loads an image, scales it by
//! `min(surface / image)`, and centers it. [`StimulusPlacement`] keeps the
//! scale factor because gaze coordinates are mapped from the monitor back into
//! stimulus pixels with the same factor.

use yet_present::{Anchor, Color, Filter, FontId, MediaId, PointPx, RectPx, Scene};

/// Background used by full-screen stimulus and calibration scenes.
pub const BACKGROUND_COLOR: Color = Color::WHITE;

/// Relative position of instruction text, as in the Python `draw_text` calls.
pub const INSTRUCTION_RELATIVE_POSITION: (f32, f32) = (0.1, 0.9);

/// Divisor applied to the shorter surface edge for instruction text size.
///
/// The Python prototype uses `min(surface) / 60`.
pub const INSTRUCTION_FONT_SIZE_DIVISOR: f32 = 60.0;

/// Font identifier expected to be registered for instruction text.
pub const INSTRUCTION_FONT: FontId = FontId::new(0);

/// Where a stimulus is drawn and how far it was scaled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StimulusPlacement {
    /// Destination rectangle in monitor-local physical pixels.
    pub dest_px: RectPx,
    /// Uniform scale factor applied to the source image.
    pub scale: f32,
}

impl StimulusPlacement {
    /// Fit `media_px` onto `surface_px`, preserving aspect ratio and centering.
    ///
    /// Returns `None` if either size has a zero dimension. Dimensions are
    /// rounded to the nearest physical pixel; the Python prototype truncates,
    /// so a placement can differ from it by at most one pixel.
    pub fn fit(media_px: (u32, u32), surface_px: (u32, u32)) -> Option<Self> {
        let (media_width, media_height) = media_px;
        let (surface_width, surface_height) = surface_px;
        if media_width == 0 || media_height == 0 || surface_width == 0 || surface_height == 0 {
            return None;
        }

        let media_width = media_width as f32;
        let media_height = media_height as f32;
        let surface_width = surface_width as f32;
        let surface_height = surface_height as f32;

        let scale = (surface_width / media_width).min(surface_height / media_height);
        let width = (media_width * scale).round();
        let height = (media_height * scale).round();

        Some(Self {
            dest_px: RectPx::new(
                ((surface_width - width) / 2.0).round(),
                ((surface_height - height) / 2.0).round(),
                width,
                height,
            ),
            scale,
        })
    }

    /// A scene that clears the surface and draws the placed stimulus.
    pub fn scene(&self, media: MediaId) -> Scene {
        let mut scene = Scene::with_capacity(2);
        scene
            .clear(BACKGROUND_COLOR)
            .image(media, self.dest_px, Filter::Linear, 1.0);
        scene
    }
}

/// A scene with a single line of instruction text at the Python position.
pub fn instruction_scene(text: impl Into<String>, surface_px: (u32, u32)) -> Scene {
    let (width, height) = surface_px;
    let size_px = (width.min(height) as f32 / INSTRUCTION_FONT_SIZE_DIVISOR).max(1.0);
    let pos_px = PointPx::new(
        INSTRUCTION_RELATIVE_POSITION.0 * width as f32,
        INSTRUCTION_RELATIVE_POSITION.1 * height as f32,
    );

    let mut scene = Scene::with_capacity(2);
    scene.clear(BACKGROUND_COLOR).text(
        pos_px,
        text,
        INSTRUCTION_FONT,
        size_px,
        Color::BLACK,
        Anchor::TopLeft,
    );
    scene
}

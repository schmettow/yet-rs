// SPDX-License-Identifier: GPL-3.0-only

//! A small, backend-neutral display list.
//!
//! The protocol deliberately owns its text and media identifiers. A presenter
//! may borrow or zero-copy internally, but scenes that cross a channel must be
//! self-contained.

use crate::geometry::{PointPx, RectPx};

/// A stable identifier for loaded media, owned by the console/engine side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MediaId(u32);

impl MediaId {
    /// Create a media identifier.
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Return the underlying value.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A stable identifier for a font registered with the presenter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontId(u32);

impl FontId {
    /// Create a font identifier.
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Return the underlying value.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// An sRGB color with straight (non-premultiplied) alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel; `0` is transparent, `255` is opaque.
    pub a: u8,
}

impl Color {
    /// Fully transparent black.
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    /// Opaque black.
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    /// Opaque white.
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    /// An opaque sRGB color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    /// An sRGB color with explicit alpha.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

/// Image sampling mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// Nearest-neighbor sampling.
    Nearest,
    /// Linear sampling.
    Linear,
}

/// Text anchor relative to the supplied position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// Top-left corner.
    TopLeft,
    /// Top edge, centered horizontally.
    TopCenter,
    /// Top-right corner.
    TopRight,
    /// Left edge, centered vertically.
    CenterLeft,
    /// Center of the text box.
    Center,
    /// Right edge, centered vertically.
    CenterRight,
    /// Bottom-left corner.
    BottomLeft,
    /// Bottom edge, centered horizontally.
    BottomCenter,
    /// Bottom-right corner.
    BottomRight,
}

/// One backend-neutral drawing instruction.
#[derive(Clone, Debug, PartialEq)]
pub enum DrawCommand {
    /// Clear the target to a color.
    Clear(Color),
    /// Draw a previously loaded image into a physical-pixel rectangle.
    Image {
        /// Media to draw.
        media: MediaId,
        /// Destination rectangle in physical pixels.
        dest_px: RectPx,
        /// Sampling mode.
        filter: Filter,
        /// Opacity multiplier in `0.0..=1.0`.
        alpha: f32,
    },
    /// Draw a filled circle, or a stroked circle when `stroke_px` is positive.
    Circle {
        /// Center in physical pixels.
        center_px: PointPx,
        /// Radius in physical pixels.
        radius_px: f32,
        /// Stroke width in physical pixels; `0.0` means filled.
        stroke_px: f32,
        /// Circle color.
        color: Color,
    },
    /// Draw a single line of text.
    Text {
        /// Anchor position in physical pixels.
        pos_px: PointPx,
        /// Owned text content.
        text: String,
        /// Font registered with the presenter.
        font: FontId,
        /// Font size in physical pixels.
        size_px: f32,
        /// Text color.
        color: Color,
        /// How `pos_px` anchors the text box.
        anchor: Anchor,
    },
    /// Draw a straight line.
    Line {
        /// Start point in physical pixels.
        from_px: PointPx,
        /// End point in physical pixels.
        to_px: PointPx,
        /// Line width in physical pixels.
        width_px: f32,
        /// Line color.
        color: Color,
    },
}

/// An ordered list of drawing instructions for one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    commands: Vec<DrawCommand>,
}

impl Scene {
    /// Create an empty scene.
    pub const fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    /// Create an empty scene with preallocated command storage.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            commands: Vec::with_capacity(capacity),
        }
    }

    /// The commands in draw order.
    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    /// Number of commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether the scene contains no commands.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Remove all commands without emitting a clear command.
    pub fn clear_commands(&mut self) {
        self.commands.clear();
    }

    /// Append an arbitrary command.
    pub fn push(&mut self, command: DrawCommand) -> &mut Self {
        self.commands.push(command);
        self
    }

    /// Append a clear command.
    pub fn clear(&mut self, color: Color) -> &mut Self {
        self.push(DrawCommand::Clear(color))
    }

    /// Append an image command.
    pub fn image(
        &mut self,
        media: MediaId,
        dest_px: RectPx,
        filter: Filter,
        alpha: f32,
    ) -> &mut Self {
        self.push(DrawCommand::Image {
            media,
            dest_px,
            filter,
            alpha,
        })
    }

    /// Append a circle command. `stroke_px == 0.0` draws a filled circle.
    pub fn circle(
        &mut self,
        center_px: PointPx,
        radius_px: f32,
        stroke_px: f32,
        color: Color,
    ) -> &mut Self {
        self.push(DrawCommand::Circle {
            center_px,
            radius_px,
            stroke_px,
            color,
        })
    }

    /// Append a text command.
    pub fn text(
        &mut self,
        pos_px: PointPx,
        text: impl Into<String>,
        font: FontId,
        size_px: f32,
        color: Color,
        anchor: Anchor,
    ) -> &mut Self {
        self.push(DrawCommand::Text {
            pos_px,
            text: text.into(),
            font,
            size_px,
            color,
            anchor,
        })
    }

    /// Append a line command.
    pub fn line(
        &mut self,
        from_px: PointPx,
        to_px: PointPx,
        width_px: f32,
        color: Color,
    ) -> &mut Self {
        self.push(DrawCommand::Line {
            from_px,
            to_px,
            width_px,
            color,
        })
    }
}

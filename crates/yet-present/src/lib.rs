// SPDX-License-Identifier: GPL-3.0-only

//! Presentation protocols and traits for yet-rs.
//!
//! This crate is the leaf of the dependency graph: it must not depend on a UI
//! toolkit, an image decoder, or a platform backend. Concrete presenters,
//! overlays, media loaders, and audio players live in sibling crates.

pub mod geometry;
pub mod protocol;
pub mod recording;
pub mod scene;
pub mod time;

pub use geometry::{PointPx, RectPx, ScreenGeometry, SizePx};
pub use protocol::{
    AudioError, AudioPlayer, ClockSource, Frame, FrameSource, FrameStamp, MediaError,
    OverlayBackend, OverlayError, PresentStats, Presenter, PresenterCommand, PresenterError,
};
pub use recording::RecordingPresenter;
pub use scene::{Anchor, Color, DrawCommand, Filter, FontId, MediaId, Scene};
pub use time::SessionTime;

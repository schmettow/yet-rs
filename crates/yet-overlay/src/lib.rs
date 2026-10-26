// SPDX-License-Identifier: GPL-3.0-only

//! Click-through desktop overlay backends for yet-rs.
//!
//! The overlay is deliberately separate from the presenter and the console. It
//! consumes an already-composed [`yet_present::Scene`] and owns its platform
//! event loop. On Linux the only backend is Wayland layer-shell; there is no
//! X11 or GNOME implementation.

pub mod raster;

#[cfg(target_os = "linux")]
mod wayland;

#[cfg(target_os = "linux")]
pub use wayland::LayerShellOverlay;

pub use yet_present::{OverlayBackend, OverlayError, Scene};

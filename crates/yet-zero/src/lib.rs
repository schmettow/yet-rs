// SPDX-License-Identifier: GPL-3.0-only

//! Library entry point for the `yet-zero` binary.
//!
//! The headless runner and the graphical console share [`driver::SessionRunner`]
//! and the same effect handling. Neither the engine nor the protocol crates
//! know about a window or an egui type. This library target exists so the
//! integration tests can drive the same code path as the binary.

pub mod camera;
pub mod console;
mod desktop_overlay;
pub mod driver;
pub mod headless;
pub mod live;
pub mod tracker;
pub mod window;

pub use console::{run_console, ConsoleOptions};
pub use desktop_overlay::{run_overlay, OverlayOptions};
pub use headless::{drive, run, HeadlessOptions, RunError, RunReport};

// SPDX-License-Identifier: GPL-3.0-only

//! Library entry point for the `yet-app` binary.
//!
//! The headless runner and the graphical console share [`driver::SessionRunner`]
//! and the same effect handling. Neither the engine nor the protocol crates
//! know about a window or an egui type.

pub mod console;
pub mod driver;
pub mod headless;
pub mod live;
pub mod tracker;
pub mod window;

pub use console::{run_console, ConsoleOptions};
pub use headless::{drive, run, HeadlessOptions, RunError, RunReport};

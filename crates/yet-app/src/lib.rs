// SPDX-License-Identifier: GPL-3.0-only

//! Library entry point for the `yet-app` binary.
//!
//! Sprint 3 only exposes the headless runner. The console and presenter
//! binaries will share the same effect handling in later sprints.

pub mod headless;
pub mod live;

pub use headless::{drive, run, HeadlessOptions, RunError, RunReport};

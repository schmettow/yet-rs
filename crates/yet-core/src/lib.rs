// SPDX-License-Identifier: GPL-3.0-only

//! UI-free experiment engine and eye-tracking core for yet-rs.
//!
//! The engine is a pure state machine: [`step`] takes an event and a session
//! timestamp, and returns effects for the application to execute. It does not
//! depend on a window, an event loop, or a clock implementation.

pub mod clock;
pub mod engine;
pub mod tracking;

pub use clock::{Clock, TestClock};
pub use engine::{
    step, ConsoleEvent, Effect, EngineError, EngineEvent, EngineState, FrameRecord, Key, Phase,
    TrialRecord,
};
pub use tracking::{CalibrationSample, GazePoint, QuadBrightness};

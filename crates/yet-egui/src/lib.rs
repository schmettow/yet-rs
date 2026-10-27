// SPDX-License-Identifier: GPL-3.0-only

//! Egui components for yet-rs.
//!
//! This crate is the only place that knows about egui, eframe or winit. It
//! provides the stimulus presenter, the experimenter console and the egui
//! desktop overlay. All experiment logic stays in [`yet_stim`] and all
//! tracker essentials stay in [`yet_core`].

pub mod console;

mod blur;
mod media;
mod overlay;
mod painter;
mod presenter;
mod view;

pub use blur::box_blur_rgba;
pub use overlay::EguiOverlay;
pub use presenter::{EguiPresenter, PresenterConfig, PresenterState, PresenterStatus};
pub use view::{show_viewport, viewport_id};

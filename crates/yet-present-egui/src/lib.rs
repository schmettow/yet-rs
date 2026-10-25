// SPDX-License-Identifier: GPL-3.0-only

//! egui/eframe stimulus presenter for yet-rs.
//!
//! [`EguiPresenter`] implements the UI-free [`yet_present::Presenter`] trait.
//! It owns decoded media, converts the backend-neutral [`yet_present::Scene`]
//! into egui shapes, and paints in a deferred viewport created by
//! [`show_viewport`]. The console and the experiment engine never see an egui
//! type, and the presenter never sees experiment state.
//!
//! Coordinates in a scene are monitor-local physical pixels. The painter
//! converts them to logical points at the single boundary where egui expects
//! points.
//!
//! Text uses egui's bundled `default_fonts`: `FontId(0)` is the proportional
//! family and every other identifier is the monospace family. Keep
//! `system_font_fallback` disabled in the eframe options to make text rendering
//! independent of the host's installed fonts.

mod blur;
mod media;
mod painter;
mod presenter;
mod view;

pub use blur::box_blur_rgba;
pub use presenter::{EguiPresenter, PresenterConfig, PresenterState, PresenterStatus};
pub use view::{show_viewport, viewport_id};

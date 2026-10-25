// SPDX-License-Identifier: GPL-3.0-only

//! The deferred egui viewport that displays stimulus scenes.
//!
//! The viewport is created from the console's root viewport with
//! [`egui::Context::show_viewport_deferred`]. A deferred viewport repaints only
//! when its own content changes, so painting the console never forces a
//! stimulus repaint. `with_monitor` is the only reliable monitor-targeting
//! mechanism on Wayland, and it requests borderless fullscreen.
//!
//! The viewport identifier is derived from the monitor index. Changing the
//! presenter monitor therefore creates a new native window; the old one is
//! removed because it is no longer shown. Reusing one identifier and sending
//! `ViewportCommand::SetMonitor` was ignored by KWin during testing.

use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::presenter::{lock_state, PresenterState};

/// The egui viewport identifier for a stimulus window on `monitor`.
pub fn viewport_id(monitor: usize) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("yet-rs-presenter", monitor))
}

/// Ask the egui context to show (or update) the stimulus viewport.
///
/// Call this once per console frame. The viewport's closure paints the current
/// scene; it runs only when that viewport needs a repaint.
pub fn show_viewport(ctx: &egui::Context, state: &Arc<Mutex<PresenterState>>) {
    let (monitor, fullscreen, viewport) = {
        let mut state = lock_state(state);
        let (monitor, fullscreen) = state.target_monitor();
        let viewport = viewport_id(monitor);
        state.set_active_viewport(viewport);
        (monitor, fullscreen, viewport)
    };

    let mut builder = egui::ViewportBuilder::default()
        .with_title("yet-rs stimulus")
        .with_app_id("yet-rs-presenter")
        .with_decorations(false)
        .with_resizable(false);

    // `with_monitor` requests borderless fullscreen on that monitor. A
    // windowed presenter without monitor targeting arrives with the overlay
    // sprint, which needs absolute window positions anyway.
    if fullscreen {
        builder = builder.with_monitor(monitor);
    }

    let state = Arc::clone(state);
    ctx.show_viewport_deferred(viewport, builder, move |ui, _class| {
        lock_state(&state).ui(ui);
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn each_monitor_has_its_own_viewport_identifier() {
        assert_ne!(super::viewport_id(0), super::viewport_id(1));
        assert_eq!(super::viewport_id(1), super::viewport_id(1));
    }
}

// SPDX-License-Identifier: GPL-3.0-only

//! An eframe viewport overlay for Windows and macOS (best effort on X11).
//!
//! The viewport is transparent, borderless, topmost and click-through. It is
//! created by [`EguiOverlay::show_viewport`] from the console's eframe loop, so
//! there is no second event loop and no extra thread.

use std::sync::{Arc, Mutex};

use eframe::egui;
use yet_present::{OverlayBackend, OverlayError, Scene};

use crate::painter;

/// A transparent overlay viewport driven by the console's eframe loop.
pub struct EguiOverlay {
    id: egui::ViewportId,
    monitor: usize,
    visible: bool,
    passthrough: bool,
    was_visible: bool,
    scene: Arc<Mutex<Scene>>,
}

impl EguiOverlay {
    /// Create an overlay for one monitor. It is hidden until `show` or
    /// `set_visible(true)`.
    pub fn new(monitor: usize) -> Self {
        Self {
            id: egui::ViewportId::from_hash_of("yet-rs-overlay"),
            monitor,
            visible: false,
            passthrough: true,
            was_visible: false,
            scene: Arc::new(Mutex::new(Scene::new())),
        }
    }

    /// Replace the overlay display list.
    pub fn set_scene(&mut self, scene: Scene) {
        *self
            .scene
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = scene;
    }

    /// Whether the overlay is currently requested.
    pub const fn is_visible(&self) -> bool {
        self.visible
    }

    /// Monitor index this overlay targets.
    pub const fn monitor(&self) -> usize {
        self.monitor
    }

    /// Whether the overlay currently accepts pointer input.
    pub const fn passthrough(&self) -> bool {
        self.passthrough
    }

    /// Create, update or hide the overlay viewport.
    ///
    /// Call this once per console frame while the console's eframe loop runs.
    pub fn show_viewport(&mut self, ctx: &egui::Context) {
        if !self.visible {
            if self.was_visible {
                ctx.send_viewport_cmd_to(self.id, egui::ViewportCommand::Visible(false));
                self.was_visible = false;
            }
            return;
        }

        if !self.was_visible {
            ctx.send_viewport_cmd_to(self.id, egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd_to(
                self.id,
                egui::ViewportCommand::MousePassthrough(self.passthrough),
            );
            self.was_visible = true;
        }

        let builder = egui::ViewportBuilder::default()
            .with_title("yet-rs overlay")
            .with_app_id("yet-rs-overlay")
            .with_transparent(true)
            .with_has_shadow(false)
            .with_decorations(false)
            .with_always_on_top()
            .with_mouse_passthrough(self.passthrough)
            .with_taskbar(false)
            .with_active(false)
            .with_monitor(self.monitor)
            .with_fullscreen(true);

        let scene = Arc::clone(&self.scene);
        ctx.show_viewport_deferred(self.id, builder, move |ui, _class| {
            let clip = ui.max_rect();
            let pixels_per_point = ui.ctx().pixels_per_point();
            let scene = scene
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            painter::paint_overlay(ui.painter(), clip, pixels_per_point, &scene);
        });
    }
}

impl OverlayBackend for EguiOverlay {
    fn show(&mut self, monitor: usize) -> Result<(), OverlayError> {
        if monitor != self.monitor {
            return Err(OverlayError::Unavailable(format!(
                "this overlay is bound to monitor {}, not {monitor}",
                self.monitor
            )));
        }
        self.visible = true;
        Ok(())
    }

    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    fn set_passthrough(&mut self, enabled: bool) -> Result<(), OverlayError> {
        self.passthrough = enabled;
        Ok(())
    }

    fn present(&mut self, scene: &Scene) -> Result<(), OverlayError> {
        self.set_scene(scene.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::EguiOverlay;
    use yet_present::{Color, OverlayBackend, PointPx, Scene};

    #[test]
    fn a_new_overlay_is_hidden_and_click_through() {
        let overlay = EguiOverlay::new(1);

        assert!(!overlay.is_visible());
        assert!(overlay.passthrough());
        assert_eq!(overlay.monitor(), 1);
    }

    #[test]
    fn present_keeps_the_scene_for_the_viewport() {
        let mut overlay = EguiOverlay::new(0);
        let mut scene = Scene::new();
        scene.clear(Color::TRANSPARENT);
        scene.circle(PointPx::new(10.0, 20.0), 3.0, 0.0, Color::WHITE);

        overlay.present(&scene).unwrap();

        assert!(overlay.show(0).is_ok());
        assert!(overlay.is_visible());
        assert!(overlay.show(1).is_err());
    }

    #[test]
    fn passthrough_can_be_disabled() {
        let mut overlay = EguiOverlay::new(0);

        overlay.set_passthrough(false).unwrap();

        assert!(!overlay.passthrough());
    }
}

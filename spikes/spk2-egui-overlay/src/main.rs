// SPDX-License-Identifier: GPL-3.0-only
//! SPK-2 (Windows/macOS path): an eframe viewport configured as a transparent,
//! always-on-top, click-through, non-activating overlay.
//!
//! This environment is X11, so a run is only a proxy. The production target is
//! Windows/macOS; compile-check it for Windows with:
//! `cargo check --target x86_64-pc-windows-msvc -p spk2-egui-overlay`.

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SPK-2 overlay")
            .with_transparent(true)
            .with_decorations(false)
            .with_always_on_top()
            .with_mouse_passthrough(true)
            .with_taskbar(false)
            .with_active(false)
            .with_monitor(1)
            .with_fullscreen(true),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "spk2-egui-overlay",
        options,
        Box::new(|_cc| Ok(Box::new(Overlay))),
    )
}

struct Overlay;

impl eframe::App for Overlay {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let center = ui.max_rect().center();
        let painter = ui.painter();
        painter.circle_filled(
            center,
            36.0,
            egui::Color32::from_rgba_unmultiplied(255, 80, 40, 190),
        );
        painter.circle_stroke(
            center,
            48.0,
            egui::Stroke::new(2.0, egui::Color32::from_white_alpha(220)),
        );

        if ctx.input(|i| i.time) > 5.0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

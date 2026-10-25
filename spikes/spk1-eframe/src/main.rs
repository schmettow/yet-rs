// SPDX-License-Identifier: GPL-3.0-only
//! SPK-1: eframe fullscreen presentation timing and HiDPI geometry.
//!
//! Run from `spikes/spk1-eframe` with `cargo run --release`.
//! The app targets monitor index 1, logs 180 frames to `spk1-frame-log.csv`,
//! switches to 150% zoom at frame 90, and exits.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::Instant;

use eframe::egui;

const FRAMES: usize = 180;
const ZOOM_FRAME: usize = 90;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SPK-1 eframe presentation")
            .with_monitor(1)
            .with_fullscreen(true)
            .with_inner_size([1280.0, 720.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "spk1-eframe",
        options,
        Box::new(|cc| Ok(Box::new(Spike::new(cc)))),
    )
}

struct Spike {
    texture: egui::TextureHandle,
    log: BufWriter<File>,
    start: Instant,
    last: Instant,
    frame: usize,
    zoom_switched: bool,
}

impl Spike {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let texture = cc.egui_ctx.load_texture(
            "spk1-checker",
            checkerboard(64, 64),
            egui::TextureOptions::NEAREST,
        );
        let mut log =
            BufWriter::new(File::create("spk1-frame-log.csv").expect("create spk1-frame-log.csv"));
        writeln!(
            log,
            "frame,elapsed_ms,dt_ms,stable_dt_ms,pixels_per_point,inner_w,inner_h,monitor_w,monitor_h"
        )
        .expect("write header");

        let now = Instant::now();
        Self {
            texture,
            log,
            start: now,
            last: now,
            frame: 0,
            zoom_switched: false,
        }
    }
}

impl eframe::App for Spike {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        let dt = now - self.last;
        self.last = now;
        let elapsed = now - self.start;

        if self.frame == ZOOM_FRAME {
            ctx.set_zoom_factor(1.5);
            self.zoom_switched = true;
        }

        let rect = ui.max_rect();
        let painter = ui.painter();
        painter.rect_filled(rect, egui::CornerRadius::ZERO, egui::Color32::BLACK);

        let image_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(256.0, 256.0));
        painter.image(
            self.texture.id(),
            image_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        painter.circle_filled(rect.center(), 40.0, egui::Color32::from_rgb(255, 80, 40));
        painter.text(
            rect.center() + egui::vec2(0.0, 96.0),
            egui::Align2::CENTER_CENTER,
            "SPK-1",
            egui::FontId::proportional(32.0),
            egui::Color32::WHITE,
        );

        let (inner_w, inner_h) = ctx
            .input(|i| i.viewport().inner_rect)
            .map_or((f32::NAN, f32::NAN), |r| (r.width(), r.height()));
        let (monitor_w, monitor_h) = ctx
            .input(|i| i.viewport().monitor_size)
            .map_or((f32::NAN, f32::NAN), |s| (s.x, s.y));
        let stable_dt = ctx.input(|i| i.stable_dt) * 1000.0;

        writeln!(
            self.log,
            "{},{:.3},{:.3},{:.3},{:.3},{:.1},{:.1},{:.1},{:.1}",
            self.frame,
            elapsed.as_secs_f64() * 1000.0,
            dt.as_secs_f64() * 1000.0,
            stable_dt,
            ctx.pixels_per_point(),
            inner_w,
            inner_h,
            monitor_w,
            monitor_h,
        )
        .expect("write frame row");
        self.log.flush().expect("flush frame log");

        self.frame += 1;
        if self.frame >= FRAMES {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

fn checkerboard(width: usize, height: usize) -> egui::ColorImage {
    let mut rgba = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let light = (x / 8 + y / 8) % 2 == 0;
            let (r, g, b) = if light { (255, 200, 0) } else { (20, 20, 60) };
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba)
}

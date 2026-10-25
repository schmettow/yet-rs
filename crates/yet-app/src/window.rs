// SPDX-License-Identifier: GPL-3.0-only

//! Fullscreen calibration window.
//!
//! The window opens on the presenter monitor, shows the 3×3 grid, and collects
//! frames automatically for each target. The participant only has to look at
//! the orange target. The terminal-driven flow remains available with
//! `--terminal` for headless environments.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use yet_core::{
    quad_brightness, Calibration, CalibrationGrid, CalibrationReport, CalibrationTarget, Camera,
    CameraInfo, EyeRegion, GazePoint, QuadBrightness,
};

use crate::live::{self, CalibrateOptions, LiveError};

const SETTLE: Duration = Duration::from_millis(800);
const MIN_COLLECT: Duration = Duration::from_millis(500);
const MAX_COLLECT: Duration = Duration::from_millis(1500);
const REPORT_HOLD: Duration = Duration::from_secs(2);
const DONE_HOLD: Duration = Duration::from_secs(4);
const FAILED_HOLD: Duration = Duration::from_secs(5);

const TARGET_GREY: egui::Color32 = egui::Color32::from_rgb(160, 160, 160);
const TARGET_ORANGE: egui::Color32 = egui::Color32::from_rgb(255, 120, 0);

/// Result of a graphical calibration.
#[derive(Clone, Debug)]
pub struct CalibrationWindowReport {
    /// Negotiated camera metadata.
    pub info: CameraInfo,
    /// Eye region used for features.
    pub roi: EyeRegion,
    /// Full-grid calibration report.
    pub report: CalibrationReport,
    /// Quick center recalibration, when run.
    pub quick_report: Option<CalibrationReport>,
    /// Number of live gaze predictions drawn.
    pub live_samples: usize,
}

/// Run the fullscreen calibration window.
///
/// `Ok(None)` means the window was closed before calibration completed.
pub fn run_calibration(
    experiment_dir: &Path,
    options: CalibrateOptions,
) -> Result<Option<CalibrationWindowReport>, LiveError> {
    let loaded = yet_core::load_experiment(experiment_dir)?;
    let camera = Camera::open(live::capture_config(&loaded.config.camera, options.device))?;
    let roi = live::eye_region(&loaded.config.camera, camera.info())?;
    let display = (loaded.config.display.width, loaded.config.display.height);
    let grid = CalibrationGrid::new(display);
    let info = camera.info().clone();
    let outcome = Arc::new(Mutex::new(None));

    let app = CalibrationApp {
        camera,
        info,
        roi,
        grid,
        display,
        calibration: Calibration::new(),
        phase: Phase::Prelude,
        options,
        outcome: Arc::clone(&outcome),
        full_report: None,
        quick_report: None,
        last_prediction: None,
        live_samples: 0,
        initial_layout_done: false,
        requested_origin: loaded.config.display.presenter_origin,
        viewport_px: (display.0 as f32, display.1 as f32),
    };

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("yet-rs calibration")
        .with_decorations(false)
        .with_inner_size([display.0 as f32, display.1 as f32]);
    viewport = match loaded.config.display.presenter_origin {
        Some([x, y]) => viewport.with_position([x as f32, y as f32]),
        None => viewport.with_fullscreen(true),
    };
    let native = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "yet-rs calibration",
        native,
        Box::new(move |_cc| Ok(Box::new(app))),
    )
    .map_err(|error| LiveError::Window(error.to_string()))?;

    let outcome = outcome
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    match outcome {
        Some(Outcome::Completed(report)) => Ok(Some(report)),
        Some(Outcome::Failed(message)) => Err(LiveError::Window(message)),
        Some(Outcome::Cancelled) | None => Ok(None),
    }
}

#[derive(Clone, Debug)]
enum Outcome {
    Completed(CalibrationWindowReport),
    Failed(String),
    Cancelled,
}

enum Phase {
    Prelude,
    Settle {
        index: usize,
        since: Instant,
    },
    Collect {
        index: usize,
        since: Instant,
        sums: [f64; 4],
        count: usize,
    },
    FullReport {
        since: Instant,
    },
    QuickSettle {
        since: Instant,
    },
    QuickCollect {
        since: Instant,
        sums: [f64; 4],
        count: usize,
    },
    QuickReport {
        since: Instant,
    },
    Live {
        until: Instant,
    },
    Done {
        since: Instant,
    },
    Failed {
        message: String,
        since: Instant,
    },
}

struct CalibrationApp {
    camera: Camera,
    info: CameraInfo,
    roi: EyeRegion,
    grid: CalibrationGrid,
    display: (u32, u32),
    calibration: Calibration,
    phase: Phase,
    options: CalibrateOptions,
    outcome: Arc<Mutex<Option<Outcome>>>,
    full_report: Option<CalibrationReport>,
    quick_report: Option<CalibrationReport>,
    last_prediction: Option<GazePoint>,
    live_samples: usize,
    initial_layout_done: bool,
    requested_origin: Option<[i32; 2]>,
    viewport_px: (f32, f32),
}

impl eframe::App for CalibrationApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let ppp = ctx.pixels_per_point().max(0.1);

        if !self.initial_layout_done {
            if let Some([x, y]) = self.requested_origin {
                // Viewport sizes and positions are logical points; the config
                // stores physical pixels. Correct after the window exists so
                // the window covers exactly the configured presenter area.
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                    x as f32 / ppp,
                    y as f32 / ppp,
                )));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                    self.display.0 as f32 / ppp,
                    self.display.1 as f32 / ppp,
                )));
            }
            self.initial_layout_done = true;
        }

        let screen = ctx.content_rect();
        self.viewport_px = (
            (screen.width() * ppp).max(1.0),
            (screen.height() * ppp).max(1.0),
        );

        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.cancel();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        if matches!(&self.phase, Phase::Prelude)
            && ctx.input(|input| input.key_pressed(egui::Key::Space))
        {
            self.phase = Phase::Settle {
                index: 0,
                since: now,
            };
        }

        self.step(now);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::WHITE))
            .show(ctx, |ui| self.draw(ui));

        let close = match &self.phase {
            Phase::Done { since } => now.duration_since(*since) >= DONE_HOLD,
            Phase::Failed { since, .. } => now.duration_since(*since) >= FAILED_HOLD,
            _ => false,
        };
        if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

impl CalibrationApp {
    fn step(&mut self, now: Instant) {
        let phase = std::mem::replace(&mut self.phase, Phase::Prelude);

        self.phase = match phase {
            Phase::Prelude => Phase::Prelude,
            Phase::Settle { index, since } => {
                if now.duration_since(since) >= SETTLE {
                    Phase::Collect {
                        index,
                        since: now,
                        sums: [0.0; 4],
                        count: 0,
                    }
                } else {
                    Phase::Settle { index, since }
                }
            }
            Phase::Collect {
                index,
                since,
                mut sums,
                mut count,
            } => {
                if let Some(features) = self.take_features() {
                    accumulate(&mut sums, features);
                    count += 1;
                }
                let elapsed = now.duration_since(since);
                if elapsed >= MIN_COLLECT
                    && (count >= self.options.samples.max(1) || elapsed >= MAX_COLLECT)
                {
                    match self.finish_target(index, sums, count, now) {
                        Ok(next) => next,
                        Err(message) => self.failed(message, now),
                    }
                } else {
                    Phase::Collect {
                        index,
                        since,
                        sums,
                        count,
                    }
                }
            }
            Phase::FullReport { since } => {
                if now.duration_since(since) >= REPORT_HOLD {
                    if self.options.quick {
                        Phase::QuickSettle { since: now }
                    } else {
                        self.start_live(now)
                    }
                } else {
                    Phase::FullReport { since }
                }
            }
            Phase::QuickSettle { since } => {
                if now.duration_since(since) >= SETTLE {
                    Phase::QuickCollect {
                        since: now,
                        sums: [0.0; 4],
                        count: 0,
                    }
                } else {
                    Phase::QuickSettle { since }
                }
            }
            Phase::QuickCollect {
                since,
                mut sums,
                mut count,
            } => {
                if let Some(features) = self.take_features() {
                    accumulate(&mut sums, features);
                    count += 1;
                }
                let elapsed = now.duration_since(since);
                if elapsed >= MIN_COLLECT
                    && (count >= self.options.samples.max(1) || elapsed >= MAX_COLLECT)
                {
                    match self.finish_quick(sums, count, now) {
                        Ok(next) => next,
                        Err(message) => self.failed(message, now),
                    }
                } else {
                    Phase::QuickCollect { since, sums, count }
                }
            }
            Phase::QuickReport { since } => {
                if now.duration_since(since) >= REPORT_HOLD {
                    self.start_live(now)
                } else {
                    Phase::QuickReport { since }
                }
            }
            Phase::Live { until } => {
                if let Some(features) = self.take_features() {
                    if let Ok(point) = self.calibration.predict(features) {
                        self.last_prediction = Some(point);
                        self.live_samples += 1;
                    }
                }
                if now >= until {
                    self.complete(now)
                } else {
                    Phase::Live { until }
                }
            }
            Phase::Done { since } => Phase::Done { since },
            Phase::Failed { message, since } => Phase::Failed { message, since },
        };
    }

    fn take_features(&mut self) -> Option<QuadBrightness> {
        let frame = self.camera.latest_frame()?;
        quad_brightness(&frame, self.roi)
    }

    fn finish_target(
        &mut self,
        index: usize,
        sums: [f64; 4],
        count: usize,
        now: Instant,
    ) -> Result<Phase, String> {
        if count == 0 {
            return Err("no camera frames arrived for the target".into());
        }
        let target = self
            .grid
            .target(index)
            .ok_or_else(|| format!("calibration target {index} is missing"))?;
        let features = mean(sums, count);
        self.calibration.record(
            GazePoint::new(
                target.normalized.x * self.viewport_px.0,
                target.normalized.y * self.viewport_px.1,
            ),
            features,
        );
        println!(
            "Target {}/{}: nw={:.2} ne={:.2} sw={:.2} se={:.2}",
            index + 1,
            self.grid.len(),
            features.nw,
            features.ne,
            features.sw,
            features.se
        );

        if index + 1 < self.grid.len() {
            Ok(Phase::Settle {
                index: index + 1,
                since: now,
            })
        } else {
            let report = self.calibration.fit().map_err(|error| error.to_string())?;
            println!(
                "Calibration: {} samples, RMSE {:.2} px",
                report.samples, report.rmse
            );
            self.full_report = Some(report);
            Ok(Phase::FullReport { since: now })
        }
    }

    fn finish_quick(
        &mut self,
        sums: [f64; 4],
        count: usize,
        now: Instant,
    ) -> Result<Phase, String> {
        if count == 0 {
            return Err("no camera frames arrived for the center target".into());
        }
        let center = self
            .grid
            .target(self.grid.len() / 2)
            .ok_or_else(|| "center calibration target is missing".to_owned())?;
        let features = mean(sums, count);
        self.calibration.record(
            GazePoint::new(
                center.normalized.x * self.viewport_px.0,
                center.normalized.y * self.viewport_px.1,
            ),
            features,
        );
        let report = self.calibration.fit().map_err(|error| error.to_string())?;
        println!(
            "Quick recalibration: {} samples, RMSE {:.2} px",
            report.samples, report.rmse
        );
        self.quick_report = Some(report);
        Ok(Phase::QuickReport { since: now })
    }

    fn start_live(&mut self, now: Instant) -> Phase {
        if self.options.skip_live || self.options.live_seconds <= 0.0 {
            self.complete(now)
        } else {
            Phase::Live {
                until: now + Duration::from_secs_f64(self.options.live_seconds),
            }
        }
    }

    fn complete(&mut self, now: Instant) -> Phase {
        if let Some(report) = self.full_report {
            self.set_outcome(Outcome::Completed(CalibrationWindowReport {
                info: self.info.clone(),
                roi: self.roi,
                report,
                quick_report: self.quick_report,
                live_samples: self.live_samples,
            }));
        }
        Phase::Done { since: now }
    }

    fn failed(&self, message: String, now: Instant) -> Phase {
        eprintln!("calibration error: {message}");
        self.set_outcome(Outcome::Failed(message.clone()));
        Phase::Failed {
            message,
            since: now,
        }
    }

    fn cancel(&self) {
        self.set_outcome(Outcome::Cancelled);
    }

    fn set_outcome(&self, outcome: Outcome) {
        let mut slot = self
            .outcome
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if slot.is_none() {
            *slot = Some(outcome);
        }
    }

    fn draw(&self, ui: &egui::Ui) {
        let rect = ui.max_rect();
        let painter = ui.painter();
        let ppp = ui.ctx().pixels_per_point().max(0.1);
        let radius = 20.0 / ppp;
        let stroke = 10.0 / ppp;
        let font_size = (self.viewport_px.0.min(self.viewport_px.1) / 60.0 / ppp).max(10.0);
        let text_pos = egui::pos2(
            rect.left() + 0.1 * rect.width(),
            rect.top() + 0.9 * rect.height(),
        );

        painter.rect_filled(rect, egui::CornerRadius::ZERO, egui::Color32::WHITE);

        let active = match &self.phase {
            Phase::Settle { index, .. } | Phase::Collect { index, .. } => Some(*index),
            Phase::QuickSettle { .. } | Phase::QuickCollect { .. } => Some(self.grid.len() / 2),
            _ => None,
        };

        let show_grid = matches!(
            &self.phase,
            Phase::Prelude
                | Phase::Settle { .. }
                | Phase::Collect { .. }
                | Phase::QuickSettle { .. }
                | Phase::QuickCollect { .. }
        );
        if show_grid {
            for target in self.grid.targets() {
                let center = target_center(rect, target);
                let color = if active == Some(target.index) {
                    TARGET_ORANGE
                } else {
                    TARGET_GREY
                };
                painter.circle_stroke(center, radius, egui::Stroke::new(stroke, color));
            }
        }

        let instruction = match &self.phase {
            Phase::Prelude => {
                "Press Space to start. Look at the orange target. ESC cancels.".to_owned()
            }
            Phase::Settle { index, .. } | Phase::Collect { index, .. } => {
                format!(
                    "Look at the orange target ({}/{})",
                    index + 1,
                    self.grid.len()
                )
            }
            Phase::FullReport { .. } => {
                if self.options.quick {
                    "Calibration complete. Quick recalibration starts…".to_owned()
                } else if self.options.skip_live {
                    "Calibration complete.".to_owned()
                } else {
                    "Calibration complete. Live gaze starts…".to_owned()
                }
            }
            Phase::QuickSettle { .. } | Phase::QuickCollect { .. } => {
                "Quick recalibration: look at the center target.".to_owned()
            }
            Phase::QuickReport { .. } => {
                "Quick recalibration complete. Live gaze starts…".to_owned()
            }
            Phase::Live { .. } => {
                "Live gaze: the red dot is the prediction. Look around. ESC stops.".to_owned()
            }
            Phase::Done { .. } => "Calibration finished. Closing…".to_owned(),
            Phase::Failed { .. } => "Calibration failed. Closing…".to_owned(),
        };
        painter.text(
            text_pos,
            egui::Align2::LEFT_TOP,
            instruction,
            egui::FontId::proportional(font_size),
            egui::Color32::BLACK,
        );

        if let Phase::FullReport { .. } = &self.phase {
            if let Some(report) = self.full_report {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("RMSE {:.2} px ({} samples)", report.rmse, report.samples),
                    egui::FontId::proportional(48.0 / ppp),
                    egui::Color32::BLACK,
                );
            }
        }
        if let Phase::QuickReport { .. } = &self.phase {
            if let Some(report) = self.quick_report {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!(
                        "Quick recalibration: RMSE {:.2} px ({} samples)",
                        report.rmse, report.samples
                    ),
                    egui::FontId::proportional(40.0 / ppp),
                    egui::Color32::BLACK,
                );
            }
        }
        if let Phase::Live { .. } = &self.phase {
            if let Some(point) = self.last_prediction {
                let x = (point.x / self.viewport_px.0).clamp(0.0, 1.0);
                let y = (point.y / self.viewport_px.1).clamp(0.0, 1.0);
                let center = egui::pos2(
                    rect.left() + x * rect.width(),
                    rect.top() + y * rect.height(),
                );
                painter.circle_filled(center, 12.0 / ppp, egui::Color32::RED);
            }
        }
        if let Phase::Failed { message, .. } = &self.phase {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                message,
                egui::FontId::proportional(28.0 / ppp),
                egui::Color32::RED,
            );
        }
    }
}

fn target_center(rect: egui::Rect, target: &CalibrationTarget) -> egui::Pos2 {
    egui::pos2(
        rect.left() + target.normalized.x * rect.width(),
        rect.top() + target.normalized.y * rect.height(),
    )
}

fn accumulate(sums: &mut [f64; 4], features: QuadBrightness) {
    for (sum, value) in sums.iter_mut().zip(features.to_array()) {
        *sum += f64::from(value);
    }
}

fn mean(sums: [f64; 4], count: usize) -> QuadBrightness {
    let count = count.max(1) as f64;
    QuadBrightness::new(
        (sums[0] / count) as f32,
        (sums[1] / count) as f32,
        (sums[2] / count) as f32,
        (sums[3] / count) as f32,
    )
}

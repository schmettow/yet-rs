// SPDX-License-Identifier: GPL-3.0-only

//! The experimenter console.
//!
//! The console shows engine state, presentation statistics, and a bounded log.
//! It never receives a scene, a media identifier, or a texture: the stimulus
//! is rendered exclusively by the presenter viewport.
//!
//! Controls follow the presentation flow: **1 · Camera adjustment**,
//! **2 · Calibration**, then **3 · Experiment**. Start requires both setup
//! steps. Adjust, Calibrate and Quick remain available during a session and
//! pause it first; Exit asks for confirmation before aborting.

use eframe::egui;
use yet_stim::Phase;

use crate::painter;
use crate::presenter::AdjustFrame;

use super::command::ExperimentCommand;
use super::state::{CameraChoice, ConsoleState, LogLevel, PresenterStatus, PreviewFrame};

/// Fallback number of monitor choices when the platform reports no monitors.
pub const MONITOR_CHOICES: usize = 4;

/// Multiplier for the experimenter console's text and spacing.
///
/// The console targets a notebook's main screen: a wide window with readable
/// controls and log text. 1.5 is 75% of the previous 2.0 scale.
pub const CONSOLE_UI_SCALE: f32 = 1.5;

/// Minimum width before the console splits into two columns.
const TWO_COLUMN_MIN_WIDTH: f32 = 860.0;

/// Displayed size of the gaze map next to the camera preview.
const GAZE_MAP_SIZE: egui::Vec2 = egui::vec2(240.0, 192.0);

/// One monitor as reported by the platform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorChoice {
    /// Zero-based index used by the configuration and the presenter.
    pub index: usize,
    /// Platform monitor name, when available.
    pub name: Option<String>,
    /// Monitor size in physical pixels.
    pub size_px: (u32, u32),
}

impl MonitorChoice {
    /// A one-based label with resolution and, when known, the output name.
    pub fn label(&self) -> String {
        let size = format!("{}×{}", self.size_px.0, self.size_px.1);
        match &self.name {
            Some(name) if !name.is_empty() => {
                format!("{} · {size} · {name}", self.index + 1)
            }
            _ => format!("{} · {size}", self.index + 1),
        }
    }
}

/// Persistent console widget state.
pub struct ConsoleUi {
    /// Whether the log panel is expanded.
    pub show_log: bool,
    preview_texture: Option<egui::TextureHandle>,
    preview_generation: u64,
    adjust_texture: Option<(u64, egui::TextureHandle)>,
    /// Unscaled egui style, captured on the first frame.
    base_style: Option<egui::Style>,
    /// Whether the exit safeguard is asking for confirmation.
    exit_confirm: bool,
}

impl std::fmt::Debug for ConsoleUi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConsoleUi")
            .field("show_log", &self.show_log)
            .field("preview_texture", &self.preview_texture.is_some())
            .field("has_adjust_texture", &self.adjust_texture.is_some())
            .field("has_base_style", &self.base_style.is_some())
            .field("exit_confirm", &self.exit_confirm)
            .finish()
    }
}

impl ConsoleUi {
    /// Create a console with the log expanded.
    pub fn new() -> Self {
        Self {
            show_log: true,
            preview_texture: None,
            preview_generation: 0,
            adjust_texture: None,
            base_style: None,
            exit_confirm: false,
        }
    }

    /// Draw the console and return the commands the experimenter requested.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut ConsoleState,
        presenter: PresenterStatus,
        monitors: &[MonitorChoice],
    ) -> Vec<ExperimentCommand> {
        self.apply_scale(ui);
        let mut commands = Vec::new();
        if self.exit_confirm && !state.can_abort() {
            self.exit_confirm = false;
        }

        ui.horizontal_wrapped(|ui| {
            ui.heading("yet-rs console");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("{} · {}", state.experiment_id, state.participant));
            });
        });
        ui.separator();

        if ui.available_width() >= TWO_COLUMN_MIN_WIDTH {
            let width = ui.available_width();
            let height = ui.available_height();
            let gap = ui.spacing().item_spacing.x;
            let left_width = (width * 0.42)
                .max(520.0)
                .min((width - 560.0 - gap).max(420.0));
            let right_width = (width - left_width - gap).max(360.0);
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(left_width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("console-controls")
                            .auto_shrink([false, false])
                            .max_height(height)
                            .show(ui, |ui| {
                                self.status_panel(ui, state);
                                ui.separator();
                                self.setup_panel(ui, state, presenter, &mut commands);
                                ui.separator();
                                self.session_panel(ui, state, presenter, &mut commands);
                                ui.separator();
                                self.progress_panel(ui, state);
                                ui.separator();
                                self.presenter_panel(ui, state, presenter, monitors);
                                ui.separator();
                                self.shortcuts(ui, state, presenter.ready, &mut commands);
                            });
                    },
                );
                ui.separator();
                ui.allocate_ui_with_layout(
                    egui::vec2(right_width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        self.preview_panel(ui, state);
                        ui.separator();
                        self.log_panel(ui, state);
                    },
                );
            });
        } else {
            self.status_panel(ui, state);
            ui.separator();
            self.setup_panel(ui, state, presenter, &mut commands);
            ui.separator();
            self.session_panel(ui, state, presenter, &mut commands);
            ui.separator();
            self.progress_panel(ui, state);
            ui.separator();
            self.presenter_panel(ui, state, presenter, monitors);
            ui.separator();
            self.preview_panel(ui, state);
            ui.separator();
            self.log_panel(ui, state);
            self.shortcuts(ui, state, presenter.ready, &mut commands);
        }

        self.exit_dialog(ui.ctx(), state, &mut commands);
        commands
    }

    fn apply_scale(&mut self, ui: &mut egui::Ui) {
        let base = self
            .base_style
            .get_or_insert_with(|| ui.style().as_ref().clone())
            .clone();
        let mut style = base;
        scale_style(&mut style, CONSOLE_UI_SCALE);
        ui.set_style(style);
    }

    fn status_panel(&self, ui: &mut egui::Ui, state: &ConsoleState) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(state.phase_label()).strong());
            ui.label(format!("{:.1} s", state.elapsed.as_secs_f64()));
            if let Some(block) = &state.current_block {
                ui.label(format!("block {block}"));
            }
            if let (Some(trial), Some(stimulus)) = (state.current_trial, &state.current_stimulus) {
                ui.label(format!("trial {} · {stimulus}", trial + 1));
            }
        });

        if let Some(error) = &state.error {
            ui.colored_label(egui::Color32::LIGHT_RED, format!("Error: {error}"));
            ui.label("The session stopped. Fix the problem named above, then restart the console.");
        }
    }

    /// Step 1 and 2: align the camera, then calibrate.
    fn setup_panel(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut ConsoleState,
        presenter: PresenterStatus,
        commands: &mut Vec<ExperimentCommand>,
    ) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("1 · Camera adjustment").strong());
            if state.adjusted {
                ui.colored_label(egui::Color32::LIGHT_GREEN, "done");
            } else {
                ui.colored_label(egui::Color32::YELLOW, "required before start");
            }
        });
        let adjust_label = if state.preview.adjust {
            "Finish adjusting"
        } else {
            "Adjust camera"
        };
        if ui
            .add_enabled(
                state.preview.adjust || state.can_adjust(),
                egui::Button::new(adjust_label),
            )
            .on_hover_text("Enlarged, mirrored camera image with the eye ROI (F3)")
            .clicked()
        {
            self.toggle_adjust(state, commands);
        }
        if state.preview.adjust {
            ui.label("Align the eye with the orange ROI, then choose “Finish adjusting”.");
        }

        ui.separator();
        self.calibration_panel(ui, state, presenter, commands);

        if state.adjusted && state.calibration.fitted {
            ui.colored_label(
                egui::Color32::LIGHT_GREEN,
                "Setup complete · ready to start",
            );
        }
    }

    /// Step 3: start, pause, continue or exit the session.
    fn session_panel(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut ConsoleState,
        presenter: PresenterStatus,
        commands: &mut Vec<ExperimentCommand>,
    ) {
        ui.label(egui::RichText::new("3 · Experiment").strong());
        let start_enabled = state.can_start() && presenter.ready;
        let start_hint = if !presenter.ready {
            "waiting for the stimulus window"
        } else if !state.adjusted {
            "adjust the camera first"
        } else if !state.calibration.fitted {
            "calibrate first"
        } else {
            "F5"
        };

        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(start_enabled, egui::Button::new("Start"))
                .on_hover_text(start_hint)
                .clicked()
            {
                commands.push(ExperimentCommand::Start);
            }
            if ui
                .add_enabled(state.can_pause(), egui::Button::new("Pause"))
                .on_hover_text("F6")
                .clicked()
            {
                commands.push(ExperimentCommand::Pause);
            }
            if ui
                .add_enabled(state.can_resume(), egui::Button::new("Continue"))
                .on_hover_text("F6 · continue after a pause")
                .clicked()
            {
                commands.push(ExperimentCommand::Resume);
            }
            let exit = ui
                .add_enabled(
                    state.can_abort(),
                    egui::Button::new(egui::RichText::new("Exit").color(egui::Color32::LIGHT_RED)),
                )
                .on_hover_text("F8 · asks before the session is cancelled");
            let exit = if state.phase == Phase::Running {
                exit.on_disabled_hover_text("Pause the session first")
            } else {
                exit
            };
            if exit.clicked() {
                self.exit_confirm = true;
            }
            if ui
                .add_enabled(state.can_reset(), egui::Button::new("Reset"))
                .on_hover_text("F4 · pause first, then return to idle")
                .clicked()
            {
                Self::reset_session(state, commands);
            }
        });

        if state.phase == Phase::Idle && !start_enabled {
            let hint = if !presenter.ready {
                "Waiting for the stimulus window."
            } else if !state.adjusted {
                "Start is disabled until the camera adjustment is finished."
            } else if !state.calibration.fitted {
                "Start is disabled until a full calibration is fitted."
            } else {
                ""
            };
            if !hint.is_empty() {
                ui.colored_label(egui::Color32::YELLOW, hint);
            }
        }
    }

    /// Toggle the adjust view, pausing a running session first.
    fn toggle_adjust(&mut self, state: &mut ConsoleState, commands: &mut Vec<ExperimentCommand>) {
        if state.preview.adjust {
            state.finish_adjust();
        } else if state.can_adjust() {
            if state.phase == Phase::Running {
                commands.push(ExperimentCommand::Pause);
            }
            state.preview.adjust = true;
        }
    }

    /// Pause a running session before an interrupting calibration.
    fn queue_interrupt(
        state: &ConsoleState,
        commands: &mut Vec<ExperimentCommand>,
        command: ExperimentCommand,
    ) {
        if state.phase == Phase::Running {
            commands.push(ExperimentCommand::Pause);
        }
        commands.push(command);
    }

    /// Return to idle; a paused session is aborted first.
    fn reset_session(state: &ConsoleState, commands: &mut Vec<ExperimentCommand>) {
        if state.phase == Phase::Paused {
            commands.push(ExperimentCommand::Abort);
        }
        commands.push(ExperimentCommand::Reset);
    }

    fn exit_dialog(
        &mut self,
        ctx: &egui::Context,
        state: &ConsoleState,
        commands: &mut Vec<ExperimentCommand>,
    ) {
        if !self.exit_confirm {
            return;
        }
        let mut confirmed = false;
        let mut dismissed = false;
        egui::Window::new("Exit session?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Sure you want to cancel the session? {}/{} trials saved.",
                    state.trials_completed, state.trials_total
                ));
                ui.label("The result file is written before the session stops.");
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("Exit and save").color(egui::Color32::LIGHT_RED),
                        ))
                        .clicked()
                    {
                        confirmed = true;
                    }
                    if ui.button("Keep running").clicked() {
                        dismissed = true;
                    }
                });
            });
        if confirmed {
            commands.push(ExperimentCommand::Abort);
            self.exit_confirm = false;
        } else if dismissed {
            self.exit_confirm = false;
        }
    }

    fn calibration_panel(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut ConsoleState,
        presenter: PresenterStatus,
        commands: &mut Vec<ExperimentCommand>,
    ) {
        let status = &state.calibration;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("2 · Calibration").strong());
            ui.label(status.phase.label());
            if status.fitted {
                ui.colored_label(egui::Color32::LIGHT_GREEN, "model fitted");
            } else {
                ui.colored_label(egui::Color32::YELLOW, "not calibrated");
            }
        });

        let can_calibrate = state.can_calibrate(presenter.ready);
        let can_quick = state.can_quick_calibrate(presenter.ready);
        let can_cancel = state.can_cancel_calibration();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(can_calibrate, egui::Button::new("Calibrate"))
                .on_hover_text("F7 · full 3×3 grid · pauses a running session")
                .clicked()
            {
                Self::queue_interrupt(state, commands, ExperimentCommand::Calibrate);
            }
            if ui
                .add_enabled(can_quick, egui::Button::new("Quick"))
                .on_hover_text("F9 · one center target · pauses a running session")
                .clicked()
            {
                Self::queue_interrupt(state, commands, ExperimentCommand::QuickCalibrate);
            }
            if ui
                .add_enabled(can_cancel, egui::Button::new("Cancel"))
                .on_hover_text("Esc")
                .clicked()
            {
                commands.push(ExperimentCommand::CancelCalibration);
            }
        });

        if !presenter.ready {
            ui.label("Calibration waits for the stimulus window.");
        }
        if let Some(error) = &state.preview.error {
            ui.colored_label(egui::Color32::LIGHT_RED, format!("Camera: {error}"));
        }

        if status.active {
            let quick = matches!(
                status.phase,
                yet_stim::CalibrationPhase::QuickSettle
                    | yet_stim::CalibrationPhase::QuickCollect
                    | yet_stim::CalibrationPhase::QuickReport
            );
            let label = if quick {
                "quick recalibration".to_owned()
            } else {
                format!("{} / {} targets", status.completed, status.total)
            };
            ui.add(egui::ProgressBar::new(status.progress()).text(label));
            if quick {
                ui.label(format!(
                    "Center target · {} frames collected",
                    status.collected
                ));
            } else if let Some(target) = status.target {
                let collecting = if status.collecting() {
                    format!(" · {} frames collected", status.collected)
                } else {
                    String::new()
                };
                ui.label(format!(
                    "Target {} of {}{}",
                    target + 1,
                    status.total,
                    collecting
                ));
            }
        }

        if let Some(report) = status.report {
            ui.label(format!(
                "Full: {} samples · RMSE {:.1} px",
                report.samples, report.rmse
            ));
        }
        if let Some(quick) = status.quick_report {
            let drift = status
                .drift_rmse
                .map(|drift| format!(" · drift {drift:+.1} px"))
                .unwrap_or_default();
            ui.label(format!(
                "Quick: {} samples · RMSE {:.1} px{drift}",
                quick.samples, quick.rmse
            ));
        }
        if let Some(error) = &status.error {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                format!("Calibration error: {error}"),
            );
            if status.fitted {
                ui.label("The previous calibration is still active. Fix the cause and retry.");
            } else {
                ui.label("No calibration is active. Check the camera and retry.");
            }
        }
    }

    fn preview_panel(&mut self, ui: &mut egui::Ui, state: &mut ConsoleState) {
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut state.preview.enabled, "Camera preview");
            ui.label("F10");
            ui.checkbox(&mut state.preview.pip_stream, "PiP camera")
                .on_hover_text("Draw the camera stream over the stimulus (presenter key P)");
            if let Some(fps) = state.preview.capture_fps {
                ui.label(format!("{fps:.1} fps capture"));
            }
            if state.preview.error.is_some() && ui.button("Retry").clicked() {
                state.preview.enabled = true;
                state.preview.retry = true;
            }
        });
        if state.preview.adjust {
            let frame = state.preview.adjust_frame.clone();
            self.show_adjust(ui, frame.as_ref());
            return;
        }
        if !state.preview.enabled {
            return;
        }

        if let Some(error) = &state.preview.error {
            ui.colored_label(egui::Color32::LIGHT_RED, format!("Camera: {error}"));
            ui.label(
                "The experiment still runs; gaze rows are unavailable until the camera returns.",
            );
        }

        let Some(frame) = &state.preview.frame else {
            ui.label("Waiting for camera frames…");
            return;
        };
        self.show_preview(ui, frame);
    }

    fn show_adjust(&mut self, ui: &mut egui::Ui, frame: Option<&AdjustFrame>) {
        let Some(frame) = frame else {
            ui.label("Waiting for camera frames…");
            return;
        };
        let expected = frame.width as usize * frame.height as usize;
        if expected == 0 || frame.data.len() != expected {
            return;
        }
        if self
            .adjust_texture
            .as_ref()
            .map(|(generation, _)| *generation)
            != Some(frame.generation)
        {
            let image = egui::ColorImage::from_gray(
                [frame.width as usize, frame.height as usize],
                &frame.data,
            );
            match &mut self.adjust_texture {
                Some((generation, texture)) => {
                    texture.set(image, egui::TextureOptions::LINEAR);
                    *generation = frame.generation;
                }
                None => {
                    self.adjust_texture = Some((
                        frame.generation,
                        ui.ctx().load_texture(
                            "yet-console-adjust",
                            image,
                            egui::TextureOptions::LINEAR,
                        ),
                    ));
                }
            }
        }
        let Some((_, texture)) = &self.adjust_texture else {
            return;
        };

        let natural = egui::vec2(frame.width as f32, frame.height as f32);
        let width = ui.available_width().clamp(320.0, 760.0);
        let scale = (width / natural.x).min(560.0 / natural.y).max(f32::EPSILON);
        let (rect, _) = ui.allocate_exact_size(natural * scale, egui::Sense::hover());
        ui.painter().image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(1.0, 0.0), egui::pos2(0.0, 1.0)),
            egui::Color32::WHITE,
        );
        painter::paint_eye_roi(
            ui.painter(),
            rect,
            painter::mirrored_roi(frame.roi),
            egui::Color32::from_rgb(255, 160, 0),
            2.5,
            "eye ROI",
            egui::FontId::proportional(14.0),
        );
        ui.label("Move the camera until the eye is inside the orange box. F3 finishes.");
    }

    fn show_preview(&mut self, ui: &mut egui::Ui, frame: &PreviewFrame) {
        if self.preview_generation != frame.generation || self.preview_texture.is_none() {
            let image = egui::ColorImage::from_gray(
                [frame.width as usize, frame.height as usize],
                &frame.data,
            );
            match &mut self.preview_texture {
                Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                None => {
                    self.preview_texture = Some(ui.ctx().load_texture(
                        "yet-console-preview",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            self.preview_generation = frame.generation;
        }

        let natural = egui::vec2(frame.width as f32, frame.height as f32);
        let scale = (480.0 / natural.x).min(320.0 / natural.y).min(1.0);
        let display = natural * scale;

        ui.horizontal(|ui| {
            if let Some(texture) = &self.preview_texture {
                ui.add(egui::Image::new(egui::load::SizedTexture::new(
                    texture.id(),
                    display,
                )));
            }

            let (rect, _) = ui.allocate_exact_size(GAZE_MAP_SIZE, egui::Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, 2.0, egui::Color32::from_gray(24));
            if let Some(quad) = frame.quad {
                let values = quad.to_array();
                let half = rect.size() / 2.0;
                for (index, value) in values.iter().enumerate() {
                    let cell = egui::Rect::from_min_size(
                        rect.min
                            + egui::vec2((index % 2) as f32 * half.x, (index / 2) as f32 * half.y),
                        half,
                    );
                    let level = value.clamp(0.0, 255.0).round() as u8;
                    painter.rect_filled(cell, 0.0, egui::Color32::from_gray(level));
                }
                let grid = egui::Stroke::new(1.0, egui::Color32::from_gray(72));
                painter.line_segment([rect.center_top(), rect.center_bottom()], grid);
                painter.line_segment([rect.left_center(), rect.right_center()], grid);
            }
            painter.rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(1.0, egui::Color32::GRAY),
                egui::StrokeKind::Inside,
            );
            if let Some(gaze) = frame.gaze_normalized {
                let point = egui::pos2(
                    rect.left() + gaze.x.clamp(0.0, 1.0) * rect.width(),
                    rect.top() + gaze.y.clamp(0.0, 1.0) * rect.height(),
                );
                painter.circle_filled(point, 5.0, egui::Color32::from_black_alpha(190));
                painter.circle_filled(point, 3.5, egui::Color32::RED);
            }

            ui.vertical(|ui| {
                ui.label(egui::RichText::new("gaze prediction").strong());
                if let Some(gaze) = frame.gaze {
                    ui.label(format!("gaze {:.0}, {:.0} px", gaze.x, gaze.y));
                } else {
                    ui.label("gaze: not calibrated");
                }
                if let Some(normalized) = frame.gaze_normalized {
                    ui.label(format!(
                        "normalized {:.3}, {:.3}",
                        normalized.x, normalized.y
                    ));
                }
                if frame.quad.is_some() {
                    ui.colored_label(egui::Color32::GRAY, "Quadbright background");
                }
            });
        });
    }

    fn progress_panel(&mut self, ui: &mut egui::Ui, state: &ConsoleState) {
        ui.add(
            egui::ProgressBar::new(state.progress())
                .show_percentage()
                .text(format!(
                    "{} / {} trials",
                    state.trials_completed, state.trials_total
                )),
        );
        ui.horizontal(|ui| {
            ui.label(format!("samples {}", state.samples));
            if state.late_trials > 0 {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    format!(
                        "late trials {} (max {:.1} ms)",
                        state.late_trials,
                        state.max_late.as_secs_f64() * 1000.0
                    ),
                );
            } else {
                ui.label("no late trials");
            }
        });
        if state.dropped_frames > 0 || state.audio_underruns > 0 || state.audio_drift_ms.is_some() {
            ui.horizontal(|ui| {
                ui.label(format!("frames dropped {}", state.dropped_frames));
                if let Some(pts) = state.media_pts {
                    ui.label(format!("media {:.2} s", pts.as_secs_f64()));
                }
                if let Some(drift) = state.audio_drift_ms {
                    ui.label(format!("audio drift {drift:+.1} ms"));
                }
                if state.audio_underruns > 0 {
                    ui.colored_label(
                        egui::Color32::YELLOW,
                        format!("underruns {}", state.audio_underruns),
                    );
                }
            });
        }
    }

    fn presenter_panel(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut ConsoleState,
        presenter: PresenterStatus,
        monitors: &[MonitorChoice],
    ) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Presenter").strong());
            if presenter.ready {
                ui.colored_label(egui::Color32::LIGHT_GREEN, "ready");
            } else {
                ui.colored_label(egui::Color32::YELLOW, "waiting for fullscreen");
            }
        });

        if presenter.ready && !presenter.focused {
            ui.colored_label(
                egui::Color32::YELLOW,
                "Stimulus window is not focused. Some compositors dim unfocused windows \
                 (KWin: Dim Inactive), which changes stimulus luminance.",
            );
        }

        if !presenter.ready {
            ui.label(format!(
                "Waiting for borderless fullscreen on monitor {}. Check display.presenter_monitor.",
                presenter.monitor
            ));
        }

        egui::Grid::new("yet-console-presenter")
            .num_columns(2)
            .spacing([12.0, 3.0])
            .show(ui, |ui| {
                ui.label("Viewport");
                ui.label(if presenter.ready {
                    format!(
                        "{}×{} px · {:.3} px/pt",
                        presenter.size_px.0, presenter.size_px.1, presenter.scale_factor
                    )
                } else {
                    "not painted yet".to_owned()
                });
                ui.end_row();

                ui.label("Paint rate");
                ui.label(match presenter.paint_hz {
                    Some(hz) => format!("{hz:.1} Hz measured"),
                    None => "measuring…".to_owned(),
                });
                ui.end_row();

                ui.label("Last frame");
                ui.label(match presenter.last_dt {
                    Some(dt) => format!("{:.1} ms", dt.as_secs_f64() * 1000.0),
                    None => "—".to_owned(),
                });
                ui.end_row();

                ui.label("Scenes");
                ui.label(format!(
                    "{} presented · {} painted · {} coalesced",
                    presenter.presents, presenter.paints, presenter.coalesced
                ));
                ui.end_row();

                ui.label("Media");
                ui.label(format!("{} loaded", presenter.media_count));
                ui.end_row();

                ui.label("Console monitor");
                ui.label(format!("{} · placed by the desktop", state.console_monitor));
                ui.end_row();
            });

        let can_configure = state.can_configure();
        ui.horizontal_wrapped(|ui| {
            ui.label("Presenter monitor");
            ui.add_enabled_ui(can_configure, |ui| {
                let selected = monitors
                    .iter()
                    .find(|monitor| monitor.index == state.presenter_monitor)
                    .map(MonitorChoice::label)
                    .unwrap_or_else(|| format!("{} (unavailable)", state.presenter_monitor + 1));
                egui::ComboBox::from_id_salt("yet-console-monitor")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        if monitors.is_empty() {
                            for index in 0..MONITOR_CHOICES {
                                ui.selectable_value(
                                    &mut state.presenter_monitor,
                                    index,
                                    format!("{} · unknown size", index + 1),
                                );
                            }
                        } else {
                            for monitor in monitors {
                                ui.selectable_value(
                                    &mut state.presenter_monitor,
                                    monitor.index,
                                    monitor.label(),
                                );
                            }
                        }
                    });
                ui.checkbox(&mut state.fullscreen, "fullscreen");
            });
        });
        if !can_configure {
            ui.label("Presenter settings are locked while the session is active.");
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Camera");
            ui.add_enabled_ui(can_configure, |ui| {
                if state.camera_devices.is_empty() {
                    ui.add(
                        egui::DragValue::new(&mut state.camera_usb_index)
                            .range(0..=31)
                            .prefix("index "),
                    );
                } else {
                    let selected = state
                        .camera_devices
                        .iter()
                        .find(|camera| camera.index == state.camera_usb_index)
                        .map(CameraChoice::label)
                        .unwrap_or_else(|| format!("{} · unavailable", state.camera_usb_index));
                    egui::ComboBox::from_id_salt("yet-console-camera")
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for camera in &state.camera_devices {
                                ui.selectable_value(
                                    &mut state.camera_usb_index,
                                    camera.index,
                                    camera.label(),
                                );
                            }
                        });
                }
                if ui.button("Scan").clicked() {
                    state.camera_rescan = true;
                }
            });
        });
        if let Some(error) = &state.camera_error {
            ui.colored_label(egui::Color32::YELLOW, format!("Camera: {error}"));
        }
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut state.follow, "Follow [F]");
            if state.follow {
                ui.label("gaze circle over the stimulus");
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(state.overlay.available, |ui| {
                ui.checkbox(&mut state.overlay.enabled, "Gaze overlay");
            });
            if state.overlay.available {
                ui.label(if state.overlay.enabled {
                    "click-through"
                } else {
                    "off"
                });
            } else if let Some(error) = &state.overlay.error {
                ui.colored_label(egui::Color32::YELLOW, format!("Overlay: {error}"));
            } else {
                ui.label("Overlay off");
            }
        });
        if state.presenter_monitor == state.console_monitor {
            ui.colored_label(
                egui::Color32::YELLOW,
                "The stimulus is on the console monitor and will cover the console.",
            );
        }
    }

    fn log_panel(&mut self, ui: &mut egui::Ui, state: &ConsoleState) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_log, "Log");
            ui.label(format!("{} lines", state.log.len()));
        });
        if !self.show_log {
            return;
        }

        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .max_height(ui.available_height().max(120.0))
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    for line in &state.log {
                        let color = match line.level {
                            LogLevel::Info => egui::Color32::GRAY,
                            LogLevel::Warning => egui::Color32::YELLOW,
                            LogLevel::Error => egui::Color32::LIGHT_RED,
                        };
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!(
                                    "[{:>8.2}] {}",
                                    line.at.as_secs_f64(),
                                    line.message
                                ))
                                .monospace()
                                .color(color),
                            )
                            .wrap(),
                        );
                    }
                });
            });
    }

    fn shortcuts(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut ConsoleState,
        presenter_ready: bool,
        commands: &mut Vec<ExperimentCommand>,
    ) {
        let (start, pause, exit, calibrate, quick, cancel, preview, follow, reset, adjust) = ui
            .input(|input| {
                (
                    input.key_pressed(egui::Key::F5),
                    input.key_pressed(egui::Key::F6),
                    input.key_pressed(egui::Key::F8),
                    input.key_pressed(egui::Key::F7),
                    input.key_pressed(egui::Key::F9),
                    input.key_pressed(egui::Key::Escape),
                    input.key_pressed(egui::Key::F10),
                    input.key_pressed(egui::Key::F),
                    input.key_pressed(egui::Key::F4),
                    input.key_pressed(egui::Key::F3),
                )
            });

        if start && state.can_start() && presenter_ready {
            commands.push(ExperimentCommand::Start);
        }
        if pause {
            if state.can_pause() {
                commands.push(ExperimentCommand::Pause);
            } else if state.can_resume() {
                commands.push(ExperimentCommand::Resume);
            }
        }
        if exit && state.can_abort() {
            self.exit_confirm = true;
        }
        if reset && state.can_reset() {
            Self::reset_session(state, commands);
        }
        if calibrate && state.can_calibrate(presenter_ready) {
            Self::queue_interrupt(state, commands, ExperimentCommand::Calibrate);
        }
        if quick && state.can_quick_calibrate(presenter_ready) {
            Self::queue_interrupt(state, commands, ExperimentCommand::QuickCalibrate);
        }
        if cancel {
            if self.exit_confirm {
                self.exit_confirm = false;
            } else if state.can_cancel_calibration() {
                commands.push(ExperimentCommand::CancelCalibration);
            }
        }
        if preview {
            state.preview.enabled = !state.preview.enabled;
        }
        if adjust {
            self.toggle_adjust(state, commands);
        }
        if follow && !ui.ctx().egui_wants_keyboard_input() {
            state.follow = !state.follow;
        }

        if state.phase == Phase::Finished {
            ui.label(
                "Session finished. Close the console window when you have copied the results.",
            );
        }
    }
}

impl Default for ConsoleUi {
    fn default() -> Self {
        Self::new()
    }
}

/// Scale text, padding and widget sizes for a large, notebook-friendly console.
fn scale_style(style: &mut egui::Style, factor: f32) {
    for font in style.text_styles.values_mut() {
        font.size *= factor;
    }

    let spacing = &mut style.spacing;
    spacing.item_spacing *= factor;
    spacing.button_padding *= factor;
    spacing.window_margin = scale_margin(spacing.window_margin, factor);
    spacing.menu_margin = scale_margin(spacing.menu_margin, factor);
    spacing.indent *= factor;
    spacing.interact_size *= factor;
    spacing.slider_width *= factor;
    spacing.slider_rail_height *= factor;
    spacing.combo_width *= factor;
    spacing.text_edit_width *= factor;
    spacing.extra_text_line_spacing *= factor;
    spacing.icon_width *= factor;
    spacing.icon_width_inner *= factor;
    spacing.icon_spacing *= factor;
    spacing.tooltip_width *= factor;
    spacing.menu_width *= factor;
    spacing.menu_spacing *= factor;
    spacing.combo_height *= factor;
    spacing.scroll.content_margin = scale_margin(spacing.scroll.content_margin, factor);
    spacing.scroll.bar_width *= factor;
    spacing.scroll.handle_min_length *= factor;
    spacing.scroll.bar_inner_margin *= factor;
    spacing.scroll.bar_outer_margin *= factor;
    spacing.scroll.floating_width *= factor;
    spacing.scroll.floating_allocated_width *= factor;
}

fn scale_margin(margin: egui::Margin, factor: f32) -> egui::Margin {
    egui::Margin {
        left: scale_i8(margin.left, factor),
        right: scale_i8(margin.right, factor),
        top: scale_i8(margin.top, factor),
        bottom: scale_i8(margin.bottom, factor),
    }
}

fn scale_i8(value: i8, factor: f32) -> i8 {
    ((f32::from(value) * factor).round() as i32).clamp(i32::from(i8::MIN), i32::from(i8::MAX)) as i8
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use eframe::egui;
    use yet_core::{GazePoint, QuadBrightness};
    use yet_stim::Phase;

    use super::{scale_style, ConsoleUi, MonitorChoice, PresenterStatus, CONSOLE_UI_SCALE};
    use crate::console::command::ExperimentCommand;
    use crate::console::state::{ConsoleState, PreviewFrame};
    use crate::presenter::AdjustFrame;

    fn monitors() -> Vec<MonitorChoice> {
        vec![
            MonitorChoice {
                index: 0,
                name: Some("DP-1".to_owned()),
                size_px: (2560, 1600),
            },
            MonitorChoice {
                index: 1,
                name: Some("DP-3".to_owned()),
                size_px: (1440, 2560),
            },
        ]
    }

    #[test]
    fn the_console_runs_a_frame_without_a_window() {
        let ctx = egui::Context::default();
        let mut state = ConsoleState::new("demo", "42", 1, 0, true, 4);
        let mut console = ConsoleUi::default();
        let presenter = PresenterStatus {
            ready: true,
            size_px: (1440, 2560),
            scale_factor: 1.0833,
            ..PresenterStatus::default()
        };
        let mut commands = Vec::new();

        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            commands = console.ui(ui, &mut state, presenter, &monitors());
        });
        output.textures_delta.clear();

        assert!(commands.is_empty());
        assert!(!output.shapes.is_empty());
    }

    #[test]
    fn a_running_session_offers_pause_but_not_exit() {
        let ctx = egui::Context::default();
        let mut state = ConsoleState::new("demo", "42", 1, 0, true, 4);
        state.phase = Phase::Running;
        let mut console = ConsoleUi::default();
        let mut commands = Vec::new();

        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            commands = console.ui(ui, &mut state, PresenterStatus::default(), &monitors());
        });
        output.textures_delta.clear();

        assert!(commands.is_empty());
        assert!(state.can_pause());
        assert!(!state.can_abort(), "the user must pause before exiting");
    }

    #[test]
    fn resetting_a_paused_session_aborts_it_first() {
        let mut state = ConsoleState::new("demo", "42", 1, 0, true, 4);
        state.phase = Phase::Paused;
        let mut commands = Vec::new();

        ConsoleUi::reset_session(&state, &mut commands);
        assert_eq!(
            commands,
            vec![ExperimentCommand::Abort, ExperimentCommand::Reset]
        );

        commands.clear();
        state.phase = Phase::Finished;
        ConsoleUi::reset_session(&state, &mut commands);
        assert_eq!(commands, vec![ExperimentCommand::Reset]);
    }

    #[test]
    fn the_console_renders_a_camera_preview() {
        let ctx = egui::Context::default();
        let mut state = ConsoleState::new("demo", "42", 1, 0, true, 4);
        state.preview.enabled = true;
        state.preview.frame = Some(PreviewFrame {
            width: 2,
            height: 2,
            data: Arc::from(vec![0_u8, 64, 128, 255]),
            generation: 1,
            quad: Some(QuadBrightness::new(40.0, 80.0, 160.0, 220.0)),
            gaze: Some(GazePoint::new(100.0, 200.0)),
            gaze_normalized: Some(GazePoint::new(0.5, 0.5)),
        });
        let mut console = ConsoleUi::default();

        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let _ = console.ui(ui, &mut state, PresenterStatus::default(), &[]);
        });
        output.textures_delta.clear();

        assert!(!output.shapes.is_empty());
    }

    #[test]
    fn the_console_renders_the_adjust_view() {
        let ctx = egui::Context::default();
        let mut state = ConsoleState::new("demo", "42", 1, 0, true, 4);
        state.preview.adjust = true;
        state.preview.adjust_frame = Some(AdjustFrame {
            width: 4,
            height: 2,
            data: Arc::from(vec![0_u8; 8]),
            generation: 1,
            roi: [0.25, 0.25, 0.5, 0.5],
        });
        let mut console = ConsoleUi::default();

        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let _ = console.ui(ui, &mut state, PresenterStatus::default(), &[]);
        });
        output.textures_delta.clear();

        assert!(!output.shapes.is_empty());
    }

    #[test]
    fn monitor_labels_are_one_based_and_show_the_output() {
        let monitors = monitors();

        assert_eq!(monitors[1].label(), "2 · 1440×2560 · DP-3");
        let unnamed = MonitorChoice {
            index: 0,
            name: None,
            size_px: (1920, 1080),
        };
        assert_eq!(unnamed.label(), "1 · 1920×1080");
    }

    #[test]
    fn console_text_and_widgets_use_the_75_percent_scale() {
        let base = egui::Style::default();
        let body = base.text_styles[&egui::TextStyle::Body].size;
        let extent = base.spacing.interact_size;
        let spacing = base.spacing.item_spacing;
        let mut scaled = base;

        scale_style(&mut scaled, CONSOLE_UI_SCALE);

        assert_eq!(CONSOLE_UI_SCALE, 1.5);
        assert_eq!(scaled.text_styles[&egui::TextStyle::Body].size, body * 1.5);
        assert_eq!(scaled.spacing.interact_size, extent * 1.5);
        assert_eq!(scaled.spacing.item_spacing, spacing * 1.5);
    }

    #[test]
    fn an_interrupting_calibration_queues_pause_first() {
        let mut state = ConsoleState::new("demo", "42", 1, 0, true, 4);
        state.phase = Phase::Running;
        state.calibration.fitted = true;
        let mut commands = Vec::new();

        ConsoleUi::queue_interrupt(&state, &mut commands, ExperimentCommand::QuickCalibrate);
        assert_eq!(
            commands,
            vec![ExperimentCommand::Pause, ExperimentCommand::QuickCalibrate]
        );

        commands.clear();
        state.phase = Phase::Paused;
        ConsoleUi::queue_interrupt(&state, &mut commands, ExperimentCommand::Calibrate);
        assert_eq!(commands, vec![ExperimentCommand::Calibrate]);
    }

    #[test]
    fn the_exit_safeguard_renders_over_a_paused_session() {
        let ctx = egui::Context::default();
        let mut state = ConsoleState::new("demo", "42", 1, 0, true, 4);
        state.phase = Phase::Paused;
        state.trials_completed = 2;
        let mut console = ConsoleUi {
            exit_confirm: true,
            ..ConsoleUi::default()
        };

        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let _ = console.ui(ui, &mut state, PresenterStatus::default(), &monitors());
        });
        output.textures_delta.clear();

        assert!(console.exit_confirm, "the safeguard waits for a decision");
        assert!(!output.shapes.is_empty());
    }

    #[test]
    fn a_broad_console_renders_two_columns() {
        let ctx = egui::Context::default();
        let mut state = ConsoleState::new("demo", "42", 1, 0, true, 4);
        let mut console = ConsoleUi::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1000.0),
            )),
            ..Default::default()
        };

        let mut output = ctx.run_ui(input, |ui| {
            let _ = console.ui(ui, &mut state, PresenterStatus::default(), &monitors());
        });
        output.textures_delta.clear();

        assert!(!output.shapes.is_empty());
    }

    #[test]
    fn a_narrow_console_falls_back_to_one_column() {
        let ctx = egui::Context::default();
        let mut state = ConsoleState::new("demo", "42", 1, 0, true, 4);
        let mut console = ConsoleUi::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(700.0, 800.0),
            )),
            ..Default::default()
        };

        let mut output = ctx.run_ui(input, |ui| {
            let _ = console.ui(ui, &mut state, PresenterStatus::default(), &monitors());
        });
        output.textures_delta.clear();

        assert!(!output.shapes.is_empty());
    }
}

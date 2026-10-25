// SPDX-License-Identifier: GPL-3.0-only

//! The experimenter console.
//!
//! The console shows engine state, presentation statistics, and a bounded log.
//! It never receives a scene, a media identifier, or a texture: the stimulus
//! is rendered exclusively by the presenter viewport.

use eframe::egui;
use yet_core::Phase;

use crate::command::ExperimentCommand;
use crate::state::{ConsoleState, LogLevel, PresenterStatus, PreviewFrame};

/// Fallback number of monitor choices when the platform reports no monitors.
pub const MONITOR_CHOICES: usize = 4;

/// Multiplier for the experimenter console's text and spacing.
///
/// The console targets a notebook's main screen: a wide window with
/// comfortably large controls and log text.
pub const CONSOLE_UI_SCALE: f32 = 2.0;

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
    /// Unscaled egui style, captured on the first frame.
    base_style: Option<egui::Style>,
}

impl std::fmt::Debug for ConsoleUi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConsoleUi")
            .field("show_log", &self.show_log)
            .field("has_preview_texture", &self.preview_texture.is_some())
            .field("has_base_style", &self.base_style.is_some())
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
            base_style: None,
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
                                self.phase_panel(ui, state, presenter, &mut commands);
                                ui.separator();
                                self.calibration_panel(ui, state, presenter, &mut commands);
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
            self.phase_panel(ui, state, presenter, &mut commands);
            ui.separator();
            self.calibration_panel(ui, state, presenter, &mut commands);
            ui.separator();
            self.progress_panel(ui, state);
            ui.separator();
            self.presenter_panel(ui, state, presenter, monitors);
            self.preview_panel(ui, state);
            ui.separator();
            self.log_panel(ui, state);
            self.shortcuts(ui, state, presenter.ready, &mut commands);
        }

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

    fn phase_panel(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut ConsoleState,
        presenter: PresenterStatus,
        commands: &mut Vec<ExperimentCommand>,
    ) {
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

        let start_enabled = state.can_start() && presenter.ready;
        let start_hint = if presenter.ready {
            "F5"
        } else {
            "waiting for the stimulus window"
        };

        ui.horizontal(|ui| {
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
                .add_enabled(state.can_resume(), egui::Button::new("Resume"))
                .on_hover_text("F6")
                .clicked()
            {
                commands.push(ExperimentCommand::Resume);
            }
            if ui
                .add_enabled(
                    state.can_abort(),
                    egui::Button::new(egui::RichText::new("Abort").color(egui::Color32::LIGHT_RED)),
                )
                .on_hover_text("F8")
                .clicked()
            {
                commands.push(ExperimentCommand::Abort);
            }
        });

        if let Some(error) = &state.error {
            ui.separator();
            ui.colored_label(egui::Color32::LIGHT_RED, format!("Error: {error}"));
            ui.label("The session stopped. Fix the problem named above, then restart the console.");
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
            ui.label(egui::RichText::new("Calibration").strong());
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
                .on_hover_text("F7 · full 3×3 grid")
                .clicked()
            {
                commands.push(ExperimentCommand::Calibrate);
            }
            if ui
                .add_enabled(can_quick, egui::Button::new("Quick"))
                .on_hover_text("F9 · one center target")
                .clicked()
            {
                commands.push(ExperimentCommand::QuickCalibrate);
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
        } else if !status.fitted && !status.active {
            ui.colored_label(
                egui::Color32::YELLOW,
                "No fitted calibration. The experiment still runs, but no gaze rows are written.",
            );
        }
        if let Some(error) = &state.preview.error {
            ui.colored_label(egui::Color32::LIGHT_RED, format!("Camera: {error}"));
        }

        if status.active {
            let quick = matches!(
                status.phase,
                yet_core::CalibrationPhase::QuickSettle
                    | yet_core::CalibrationPhase::QuickCollect
                    | yet_core::CalibrationPhase::QuickReport
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
        ui.horizontal(|ui| {
            ui.checkbox(&mut state.preview.enabled, "Camera preview");
            ui.label("F10");
            if let Some(fps) = state.preview.capture_fps {
                ui.label(format!("{fps:.1} fps capture"));
            }
            if state.preview.error.is_some() && ui.button("Retry").clicked() {
                state.preview.enabled = true;
                state.preview.retry = true;
            }
        });
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
            painter.rect_filled(rect, 2.0, egui::Color32::from_gray(32));
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
                painter.circle_filled(point, 4.0, egui::Color32::RED);
            }

            ui.vertical(|ui| {
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
        let (start, pause, abort, calibrate, quick, cancel, preview) = ui.input(|input| {
            (
                input.key_pressed(egui::Key::F5),
                input.key_pressed(egui::Key::F6),
                input.key_pressed(egui::Key::F8),
                input.key_pressed(egui::Key::F7),
                input.key_pressed(egui::Key::F9),
                input.key_pressed(egui::Key::Escape),
                input.key_pressed(egui::Key::F10),
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
        if abort && state.can_abort() {
            commands.push(ExperimentCommand::Abort);
        }
        if calibrate && state.can_calibrate(presenter_ready) {
            commands.push(ExperimentCommand::Calibrate);
        }
        if quick && state.can_quick_calibrate(presenter_ready) {
            commands.push(ExperimentCommand::QuickCalibrate);
        }
        if cancel && state.can_cancel_calibration() {
            commands.push(ExperimentCommand::CancelCalibration);
        }
        if preview {
            state.preview.enabled = !state.preview.enabled;
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
    use yet_core::{GazePoint, Phase};

    use super::{scale_style, ConsoleUi, MonitorChoice, PresenterStatus, CONSOLE_UI_SCALE};
    use crate::state::{ConsoleState, PreviewFrame};

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
    fn a_running_session_offers_pause_and_abort() {
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
        assert!(state.can_abort());
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
    fn console_text_and_widgets_are_doubled() {
        let base = egui::Style::default();
        let body = base.text_styles[&egui::TextStyle::Body].size;
        let extent = base.spacing.interact_size;
        let spacing = base.spacing.item_spacing;
        let mut scaled = base;

        scale_style(&mut scaled, CONSOLE_UI_SCALE);

        assert_eq!(scaled.text_styles[&egui::TextStyle::Body].size, body * 2.0);
        assert_eq!(scaled.spacing.interact_size, extent * 2.0);
        assert_eq!(scaled.spacing.item_spacing, spacing * 2.0);
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

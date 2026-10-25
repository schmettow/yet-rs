// SPDX-License-Identifier: GPL-3.0-only

//! The experimenter console.
//!
//! The console shows engine state, presentation statistics, and a bounded log.
//! It never receives a scene, a media identifier, or a texture: the stimulus
//! is rendered exclusively by the presenter viewport.

use eframe::egui;
use yet_core::Phase;

use crate::command::ExperimentCommand;
use crate::state::{ConsoleState, LogLevel, PresenterStatus};

/// Number of monitor indices offered in the presenter-monitor selector.
pub const MONITOR_CHOICES: usize = 4;

/// Persistent console widget state.
#[derive(Debug)]
pub struct ConsoleUi {
    /// Whether the log panel is expanded.
    pub show_log: bool,
}

impl ConsoleUi {
    /// Create a console with the log expanded.
    pub fn new() -> Self {
        Self { show_log: true }
    }

    /// Draw the console and return the commands the experimenter requested.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut ConsoleState,
        presenter: PresenterStatus,
    ) -> Vec<ExperimentCommand> {
        let mut commands = Vec::new();

        ui.horizontal(|ui| {
            ui.heading("yet-rs console");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("{} · {}", state.experiment_id, state.participant));
            });
        });
        ui.separator();

        self.phase_panel(ui, state, presenter, &mut commands);
        ui.separator();
        self.progress_panel(ui, state);
        ui.separator();
        self.presenter_panel(ui, state, presenter);
        ui.separator();
        self.log_panel(ui, state);

        self.shortcuts(ui, state, presenter.ready, &mut commands);
        commands
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
    }

    fn presenter_panel(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut ConsoleState,
        presenter: PresenterStatus,
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
                ui.label(format!(
                    "{} (window placement is controlled by the desktop)",
                    state.console_monitor
                ));
                ui.end_row();
            });

        let can_configure = state.can_configure();
        ui.horizontal(|ui| {
            ui.label("Presenter monitor");
            ui.add_enabled_ui(can_configure, |ui| {
                egui::ComboBox::from_id_salt("yet-console-monitor")
                    .selected_text(state.presenter_monitor.to_string())
                    .show_ui(ui, |ui| {
                        for index in 0..MONITOR_CHOICES {
                            ui.selectable_value(
                                &mut state.presenter_monitor,
                                index,
                                format!("{index}"),
                            );
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
                for line in &state.log {
                    let color = match line.level {
                        LogLevel::Info => egui::Color32::GRAY,
                        LogLevel::Warning => egui::Color32::YELLOW,
                        LogLevel::Error => egui::Color32::LIGHT_RED,
                    };
                    ui.label(
                        egui::RichText::new(format!(
                            "[{:>8.2}] {}",
                            line.at.as_secs_f64(),
                            line.message
                        ))
                        .monospace()
                        .color(color),
                    );
                }
            });
    }

    fn shortcuts(
        &mut self,
        ui: &mut egui::Ui,
        state: &ConsoleState,
        presenter_ready: bool,
        commands: &mut Vec<ExperimentCommand>,
    ) {
        let (start, pause, abort) = ui.input(|input| {
            (
                input.key_pressed(egui::Key::F5),
                input.key_pressed(egui::Key::F6),
                input.key_pressed(egui::Key::F8),
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

#[cfg(test)]
mod tests {
    use eframe::egui;
    use yet_core::Phase;

    use super::{ConsoleUi, PresenterStatus};
    use crate::state::ConsoleState;

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
            commands = console.ui(ui, &mut state, presenter);
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
            commands = console.ui(ui, &mut state, PresenterStatus::default());
        });
        output.textures_delta.clear();

        assert!(commands.is_empty());
        assert!(state.can_pause());
        assert!(state.can_abort());
    }
}

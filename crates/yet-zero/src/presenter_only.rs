// SPDX-License-Identifier: GPL-3.0-only

//! Presenter-only mode: a stimulus window without the experimenter console.
//!
//! This is the mode for checking the eye tracker with immediate visual
//! feedback. The stimulus window is the root viewport, so there is no separate
//! console window. Keys:
//!
//! * `F` toggle the follow circle
//! * `Space` start, pause or resume
//! * `F7` full calibration, `F9` quick recalibration
//! * `F8` abort, `Esc` quit
//!
//! The circle is composed into a temporary scene for painting only; it never
//! changes the trial scene or the result CSV.

use std::path::Path;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use yet_egui::console::{ConsoleState, ExperimentCommand, LogLevel};
use yet_egui::{EguiPresenter, PresenterConfig};
use yet_stim::media::open_player;
use yet_stim::Phase;

use crate::console::{lock_console, plan_has_audio, spawn_controller};
use crate::driver::{prepare, RunError};

/// Options for a presenter-only run.
#[derive(Clone, Copy, Debug)]
pub struct PresenterOptions<'a> {
    /// Experiment directory.
    pub experiment_dir: &'a Path,
    /// Participant identifier passed to the result CSV.
    pub participant: Option<&'a str>,
    /// Result CSV override.
    pub output: Option<&'a Path>,
    /// Camera index override.
    pub device: Option<usize>,
}

impl<'a> PresenterOptions<'a> {
    /// Create options for an experiment directory.
    pub fn new(experiment_dir: &'a Path) -> Self {
        Self {
            experiment_dir,
            participant: None,
            output: None,
            device: None,
        }
    }

    /// Set the participant identifier.
    pub fn participant(mut self, participant: &'a str) -> Self {
        self.participant = Some(participant);
        self
    }

    /// Override the result CSV path.
    pub fn output(mut self, output: &'a Path) -> Self {
        self.output = Some(output);
        self
    }

    /// Override the camera index.
    pub fn device(mut self, device: usize) -> Self {
        self.device = Some(device);
        self
    }
}

/// Open the stimulus window without a console and run until it closes.
pub fn run_presenter(options: PresenterOptions<'_>) -> Result<(), RunError> {
    let prepared = prepare(options.experiment_dir, options.participant, options.output)?;
    let experiment_id = prepared.loaded.experiment.id.clone();
    let display = prepared.loaded.config.display.clone();
    let mut camera_config = prepared.loaded.config.camera.clone();
    let initial_device = options.device.unwrap_or(camera_config.usb_index);
    camera_config.usb_index = initial_device;
    let audio_config = prepared.loaded.config.audio.clone();
    let trials = prepared.loaded.plan.trial_count();
    let participant = prepared.sink.identity().participant.clone();

    let mut audio = open_player(
        audio_config.enabled && plan_has_audio(&prepared.loaded.plan),
        &audio_config.device,
    )?;
    audio.set_output_latency(Duration::from_millis(u64::from(
        audio_config.output_latency_ms,
    )));
    let presenter = EguiPresenter::with_audio(
        PresenterConfig::new(
            display.presenter_monitor,
            display.fullscreen,
            (display.width, display.height),
            1.0,
        ),
        audio,
        audio_config.volume,
    );

    let shared = Arc::new(Mutex::new(ConsoleState::new(
        &experiment_id,
        participant,
        display.presenter_monitor,
        display.console_monitor,
        display.fullscreen,
        trials,
    )));
    {
        let mut console = lock_console(&shared);
        console.camera_usb_index = initial_device;
        console.log(
            LogLevel::Info,
            format!(
                "presenter-only: {} trials; result `{}`",
                trials,
                prepared.result_file.display()
            ),
        );
        console.log(
            LogLevel::Info,
            "keys: F follow · Space start/pause · F7 calibrate · F9 quick · F8 abort · Esc quit",
        );
    }

    let (commands, controller) = spawn_controller(
        prepared.loaded.plan,
        prepared.sink,
        presenter.clone(),
        Arc::clone(&shared),
        camera_config,
        None,
        0,
    )?;
    let shutdown = commands.clone();

    let app = PresenterOnlyApp {
        shared,
        presenter,
        commands,
    };
    let native = eframe::NativeOptions {
        viewport: presenter_viewport(
            display.presenter_monitor,
            display.fullscreen,
            (display.width, display.height),
        )
        .with_title(format!("yet-zero presenter · {experiment_id}")),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::WgpuConfiguration::default()
            .with_surface_config(eframe::SurfaceConfig::LOW_LATENCY),
        system_font_fallback: false,
        persist_window: false,
        ..Default::default()
    };
    let result = eframe::run_native(
        "yet-zero presenter",
        native,
        Box::new(move |_cc| Ok(Box::new(app))),
    )
    .map_err(|error| RunError::Window(error.to_string()));
    let _ = shutdown.send(ExperimentCommand::Shutdown);
    let _ = controller.join();
    result
}

fn presenter_viewport(
    monitor: usize,
    fullscreen: bool,
    size_px: (u32, u32),
) -> egui::ViewportBuilder {
    let builder = egui::ViewportBuilder::default()
        .with_app_id("yet-rs-presenter")
        .with_decorations(false)
        .with_resizable(false)
        .with_fullscreen(fullscreen);
    if fullscreen {
        builder.with_monitor(monitor)
    } else {
        builder.with_inner_size([size_px.0 as f32, size_px.1 as f32])
    }
}

struct PresenterOnlyApp {
    shared: Arc<Mutex<ConsoleState>>,
    presenter: EguiPresenter,
    commands: Sender<ExperimentCommand>,
}

impl eframe::App for PresenterOnlyApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.presenter.show_root(ui);
        self.draw_status(ui);

        let (follow, space, calibrate, quick, abort, quit) = ctx.input(|input| {
            (
                input.key_pressed(egui::Key::F),
                input.key_pressed(egui::Key::Space),
                input.key_pressed(egui::Key::F7),
                input.key_pressed(egui::Key::F9),
                input.key_pressed(egui::Key::F8),
                input.key_pressed(egui::Key::Escape),
            )
        });

        if follow {
            let mut console = lock_console(&self.shared);
            console.follow = !console.follow;
        }
        if space {
            let phase = lock_console(&self.shared).phase;
            let command = match phase {
                Phase::Idle => Some(ExperimentCommand::Start),
                Phase::Running => Some(ExperimentCommand::Pause),
                Phase::Paused => Some(ExperimentCommand::Resume),
                _ => None,
            };
            if let Some(command) = command {
                let _ = self.commands.send(command);
            }
        }
        if calibrate {
            let _ = self.commands.send(ExperimentCommand::Calibrate);
        }
        if quick {
            let _ = self.commands.send(ExperimentCommand::QuickCalibrate);
        }
        if abort {
            let _ = self.commands.send(ExperimentCommand::Abort);
        }
        if quit {
            let _ = self.commands.send(ExperimentCommand::Shutdown);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Keep the follow circle and calibration scenes smooth while the
        // controller updates the shared presenter state.
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

impl PresenterOnlyApp {
    /// A small status strip outside a running stimulus.
    ///
    /// Presenter-only mode has no console, so camera and calibration errors
    /// would otherwise be invisible. The strip hides while a trial runs and
    /// reappears when the session stops or an error is present.
    fn draw_status(&self, ui: &egui::Ui) {
        let console = lock_console(&self.shared);
        let error = console
            .error
            .as_deref()
            .or(console.camera_error.as_deref())
            .or(console.calibration.error.as_deref());
        if console.phase == Phase::Running && error.is_none() {
            return;
        }

        let calibration = if console.calibration.active {
            format!("calibration: {}", console.calibration.phase.label())
        } else if console.calibration.fitted {
            "calibrated".to_owned()
        } else {
            "not calibrated".to_owned()
        };
        let follow = if console.follow {
            "follow on"
        } else {
            "follow off"
        };
        let line = format!(
            "{} · {calibration} · {follow}\nF7 calibrate · F9 quick · F follow · Space start/pause · F8 abort · Esc quit",
            console.phase.label()
        );

        let origin = ui.max_rect().min + egui::vec2(18.0, 18.0);
        let width = (ui.max_rect().width() - 36.0).min(760.0);
        let rect = egui::Rect::from_min_size(origin, egui::vec2(width, 74.0));
        let painter = ui.painter();
        painter.rect_filled(rect, 6.0, egui::Color32::from_black_alpha(190));
        painter.text(
            origin + egui::vec2(12.0, 10.0),
            egui::Align2::LEFT_TOP,
            line,
            egui::FontId::proportional(18.0),
            egui::Color32::WHITE,
        );
        if let Some(error) = error {
            painter.text(
                origin + egui::vec2(12.0, 52.0),
                egui::Align2::LEFT_TOP,
                error,
                egui::FontId::proportional(16.0),
                egui::Color32::from_rgb(255, 120, 120),
            );
        }
    }
}

impl Drop for PresenterOnlyApp {
    fn drop(&mut self) {
        let _ = self.commands.send(ExperimentCommand::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui;

    use super::presenter_viewport;

    #[test]
    fn a_fullscreen_presenter_targets_one_monitor() {
        let builder = presenter_viewport(1, true, (900, 900));

        assert_eq!(builder.fullscreen, Some(true));
        assert_eq!(builder.monitor, Some(1));
    }

    #[test]
    fn a_windowed_presenter_uses_the_configured_size() {
        let builder = presenter_viewport(1, false, (900, 900));

        assert_eq!(builder.fullscreen, Some(false));
        assert_eq!(builder.monitor, None);
        assert_eq!(builder.inner_size, Some(egui::vec2(900.0, 900.0)));
    }
}

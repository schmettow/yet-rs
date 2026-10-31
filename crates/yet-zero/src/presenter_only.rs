// SPDX-License-Identifier: GPL-3.0-only

//! Presenter-only mode: a linear experiment flow without a console.
//!
//! The window is the root viewport and walks through
//! `Welcome → Adjust → Calibration → Stimuli → Debrief`. Presenter mode always
//! starts fullscreen on the presenter monitor. The primary controls are
//! **Space** (forward), **Backspace** (back), **F11** (toggle fullscreen;
//! `Cmd+Ctrl+F` on macOS) and **Esc** (leave fullscreen). Welcome and debrief
//! text comes from `Intro.md` and `Debrief.md` in the workspace root, with
//! built-in defaults when absent.
//!
//! Backspace in the stimulus step pauses the session, so the experimenter can
//! return to calibration (and from there to the adjust view) and continue the
//! same session afterwards.
//!
//! `F` (follow) and `P` (camera PiP) remain available as diagnostic keys but
//! are not part of the linear flow. Escape never cancels a session; stopping
//! the program is deliberately a Backspace walk:
//! Stimuli → Calibration → Adjust → “cancel?” → exit.

use std::path::Path;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use yet_egui::console::{adjust_eye_region, EYE_REGION_MOVE_STEP, EYE_REGION_SIZE_STEP};
use yet_egui::console::{ConsoleState, ExperimentCommand, LogLevel};
use yet_egui::markdown::show_markdown;
use yet_egui::{EguiPresenter, PresenterConfig};
use yet_stim::media::open_player;
use yet_stim::Phase;

use crate::console::{lock_console, plan_has_audio, spawn_controller};
use crate::driver::{prepare, RunError};

/// Hint shown under the enlarged camera view in the linear flow.
const ADJUST_HINT: &str =
    "Step 2 · Adjust · +/− size · arrow keys move · Space continue · Backspace back · F11 fullscreen";

/// Built-in welcome page when the workspace has no `Intro.md`.
const DEFAULT_INTRO: &str = "\
# Welcome

Thank you for taking part. The experimenter will help you get settled.

Press **Space** to begin.";

/// Built-in debrief page when the workspace has no `Debrief.md`.
const DEFAULT_DEBRIEF: &str = "\
# Thank you!

You have completed the session. The experimenter will help you shortly.

Press **Space** to close.";

/// One step of the linear presenter flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Welcome,
    Adjust,
    Calibrate,
    Stimuli,
    Debrief,
    ConfirmCancel,
}

impl Step {
    /// One-based position for the step banner.
    const fn index(self) -> usize {
        match self {
            Self::Welcome => 1,
            Self::Adjust => 2,
            Self::Calibrate => 3,
            Self::Stimuli => 4,
            Self::Debrief => 5,
            Self::ConfirmCancel => 6,
        }
    }

    /// Label for the step banner.
    const fn label(self) -> &'static str {
        match self {
            Self::Welcome => "welcome",
            Self::Adjust => "adjust",
            Self::Calibrate => "calibration",
            Self::Stimuli => "stimuli",
            Self::Debrief => "debrief",
            Self::ConfirmCancel => "cancel",
        }
    }
}

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
    /// Start with the follow circle enabled.
    pub follow: bool,
    /// Start with the camera picture-in-picture enabled.
    pub pip_stream: bool,
    /// Start directly in the adjust step.
    pub adjust: bool,
}

impl<'a> PresenterOptions<'a> {
    /// Create options for an experiment directory.
    pub fn new(experiment_dir: &'a Path) -> Self {
        Self {
            experiment_dir,
            participant: None,
            output: None,
            device: None,
            follow: false,
            pip_stream: false,
            adjust: false,
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

    /// Start with the follow circle enabled.
    pub fn follow(mut self, follow: bool) -> Self {
        self.follow = follow;
        self
    }

    /// Start with the camera picture-in-picture enabled.
    pub fn pip_stream(mut self, enabled: bool) -> Self {
        self.pip_stream = enabled;
        self
    }

    /// Start directly in the adjust step.
    pub fn adjust(mut self, enabled: bool) -> Self {
        self.adjust = enabled;
        self
    }
}

/// Open the stimulus window and run the linear experiment flow.
pub fn run_presenter(options: PresenterOptions<'_>) -> Result<(), RunError> {
    let prepared = prepare(options.experiment_dir, options.participant, options.output)?;
    let experiment_id = prepared.loaded.experiment.id.clone();
    let mut display = prepared.loaded.config.display.clone();
    // Presenter-only runs start fullscreen on the presenter monitor. Esc leaves
    // fullscreen without touching the session, so a startled participant can
    // always get back to the desktop; F11 re-enters it.
    display.fullscreen = true;
    let mut camera_config = prepared.loaded.config.camera.clone();
    let initial_device = options.device.unwrap_or(camera_config.usb_index);
    camera_config.usb_index = initial_device;
    let audio_config = prepared.loaded.config.audio.clone();
    let trials = prepared.loaded.plan.trial_count();
    let participant = prepared.sink.identity().participant.clone();
    let intro = prepared
        .loaded
        .intro
        .clone()
        .unwrap_or_else(|| DEFAULT_INTRO.to_owned());
    let debrief = prepared
        .loaded
        .debrief
        .clone()
        .unwrap_or_else(|| DEFAULT_DEBRIEF.to_owned());

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
    let start_step = start_step(options.adjust);
    {
        let mut console = lock_console(&shared);
        console.camera_usb_index = initial_device;
        console.follow = options.follow;
        console.preview.pip_stream = options.pip_stream;
        console.preview.adjust = start_step == Step::Adjust;
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
            "keys: Space forward · Backspace back · F11 fullscreen · Esc leave fullscreen",
        );
    }
    if start_step == Step::Adjust {
        presenter.set_adjust_hint(Some(ADJUST_HINT.to_owned()));
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
        step: start_step,
        intro,
        debrief,
        calibrate_started: false,
        calibration_seen_active: false,
        close_requested: false,
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

/// The first step of the linear flow.
const fn start_step(adjust: bool) -> Step {
    if adjust {
        Step::Adjust
    } else {
        Step::Welcome
    }
}

/// What Backspace does in the linear presenter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackAction {
    /// Welcome and debrief have no previous step.
    Stay,
    /// Move to another step of the flow.
    Go(Step),
    /// Enter calibration and start the grid immediately.
    Calibrate,
    /// Close the presenter and flush the result.
    Exit,
}

/// The Backspace walk, kept pure so the flow survives UI changes.
///
/// `Stimuli` deliberately maps to `Calibrate` rather than the instruction
/// page: Backspace must land on the grid itself.
const fn back_action(step: Step) -> BackAction {
    match step {
        Step::Welcome | Step::Debrief => BackAction::Stay,
        Step::Adjust => BackAction::Go(Step::ConfirmCancel),
        Step::Calibrate => BackAction::Go(Step::Adjust),
        Step::Stimuli => BackAction::Calibrate,
        Step::ConfirmCancel => BackAction::Exit,
    }
}

/// Escape never cancels a session; it only leaves fullscreen.
const fn leaves_fullscreen(fullscreen: Option<bool>) -> bool {
    matches!(fullscreen, Some(true))
}

/// Human-readable progress for the cancel confirmation.
fn saved_label(completed: usize, total: usize) -> String {
    format!("{completed}/{total} trials saved")
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
    step: Step,
    intro: String,
    debrief: String,
    calibrate_started: bool,
    calibration_seen_active: bool,
    close_requested: bool,
}

impl eframe::App for PresenterOnlyApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let (space, backspace, toggle_fullscreen, escape, follow, pip_stream) =
            ctx.input(|input| {
                let pressed = |key| {
                    input.events.iter().any(|event| {
                        matches!(
                            event,
                            egui::Event::Key {
                                key: event_key,
                                pressed: true,
                                repeat: false,
                                ..
                            } if *event_key == key
                        )
                    })
                };
                let mac_fullscreen =
                    input.modifiers.command && input.modifiers.ctrl && pressed(egui::Key::F);
                (
                    pressed(egui::Key::Space),
                    pressed(egui::Key::Backspace),
                    pressed(egui::Key::F11) || mac_fullscreen,
                    pressed(egui::Key::Escape),
                    pressed(egui::Key::F),
                    pressed(egui::Key::P),
                )
            });

        if toggle_fullscreen {
            let fullscreen = ctx
                .input(|input| input.viewport().fullscreen)
                .unwrap_or(false);
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
        }
        if escape {
            let fullscreen = ctx.input(|input| input.viewport().fullscreen);
            if leaves_fullscreen(fullscreen) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            }
        }
        if follow {
            let mut console = lock_console(&self.shared);
            console.follow = !console.follow;
        }
        if pip_stream {
            let mut console = lock_console(&self.shared);
            console.preview.pip_stream = !console.preview.pip_stream;
        }
        if self.step == Step::Adjust {
            let (left, right, up, down, grow, shrink) = ctx.input(|input| {
                (
                    input.key_pressed(egui::Key::ArrowLeft),
                    input.key_pressed(egui::Key::ArrowRight),
                    input.key_pressed(egui::Key::ArrowUp),
                    input.key_pressed(egui::Key::ArrowDown),
                    input.key_pressed(egui::Key::Plus) || input.key_pressed(egui::Key::Equals),
                    input.key_pressed(egui::Key::Minus),
                )
            });
            if left || right || up || down || grow || shrink {
                let dx = (right as i32 - left as i32) as f32 * EYE_REGION_MOVE_STEP;
                let dy = (down as i32 - up as i32) as f32 * EYE_REGION_MOVE_STEP;
                let dsize = (grow as i32 - shrink as i32) as f32 * EYE_REGION_SIZE_STEP;
                let mut console = lock_console(&self.shared);
                let region = console.preview.adjust_frame.as_mut().map(|frame| {
                    let region = adjust_eye_region(frame.roi, dx, dy, dsize);
                    frame.roi = region;
                    region
                });
                if let Some(region) = region {
                    console.eye_region_request = Some(region);
                }
            }
        }
        if space {
            self.forward();
        }
        if backspace {
            self.backward();
        }
        if self.close_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.sync_step();
        self.advance_if_finished();

        self.presenter.show_root(ui);
        self.draw_step(ui);

        // Keep the adjust view, calibration and paused states responsive.
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

impl PresenterOnlyApp {
    /// Enter the next linear step.
    fn forward(&mut self) {
        match self.step {
            Step::Welcome => self.enter(Step::Adjust),
            Step::Adjust => self.enter(Step::Calibrate),
            Step::Calibrate => self.forward_calibration(),
            Step::Stimuli => self.forward_stimuli(),
            Step::Debrief => self.close(),
            Step::ConfirmCancel => self.enter(Step::Adjust),
        }
    }

    /// Go back one step. From the stimulus step this pauses the session.
    fn backward(&mut self) {
        match back_action(self.step) {
            BackAction::Stay => {}
            BackAction::Go(Step::Adjust) => {
                if lock_console(&self.shared).calibration.active {
                    let _ = self.commands.send(ExperimentCommand::CancelCalibration);
                }
                self.enter(Step::Adjust);
            }
            BackAction::Go(step) => self.enter(step),
            BackAction::Calibrate => {
                // Cancel an in-flight quick recalibration first, then pause.
                let _ = self.commands.send(ExperimentCommand::CancelCalibration);
                let _ = self.commands.send(ExperimentCommand::Pause);
                self.enter(Step::Calibrate);
                // Backspace from a stimulus lands on the calibration grid, not
                // on the instruction page.
                self.begin_calibration();
            }
            BackAction::Exit => self.close(),
        }
    }

    /// Start or resume the session when the stimulus step is entered.
    fn enter(&mut self, step: Step) {
        let adjust = step == Step::Adjust;
        {
            let mut console = lock_console(&self.shared);
            if adjust {
                console.preview.adjust = true;
            } else {
                console.finish_adjust();
            }
        }
        self.presenter
            .set_adjust_hint(adjust.then(|| ADJUST_HINT.to_owned()));
        self.calibrate_started = false;
        self.calibration_seen_active = false;
        self.step = step;

        if step == Step::ConfirmCancel && lock_console(&self.shared).phase == Phase::Running {
            let _ = self.commands.send(ExperimentCommand::Pause);
        }

        if step == Step::Stimuli {
            let phase = lock_console(&self.shared).phase;
            match phase {
                Phase::Idle => {
                    let _ = self.commands.send(ExperimentCommand::Start);
                }
                Phase::Paused => {
                    let _ = self.commands.send(ExperimentCommand::Resume);
                }
                Phase::Finished => self.enter(Step::Debrief),
                Phase::Running | Phase::Aborted => {}
            }
        }
    }

    /// Space in the calibration step: start a run, or continue after it.
    fn forward_calibration(&mut self) {
        let (active, fitted, error, phase) = {
            let console = lock_console(&self.shared);
            (
                console.calibration.active,
                console.calibration.fitted,
                console.calibration.error.is_some(),
                console.phase,
            )
        };
        if active {
            return;
        }
        if phase == Phase::Running {
            let _ = self.commands.send(ExperimentCommand::Pause);
            return;
        }
        if self.calibration_seen_active {
            if error {
                // Failed run: the next Space retries.
                self.calibrate_started = false;
                self.calibration_seen_active = false;
            } else if fitted {
                self.enter(Step::Stimuli);
            } else {
                self.calibrate_started = false;
                self.calibration_seen_active = false;
            }
            return;
        }
        // Start, or re-send if the command has not been picked up yet.
        self.begin_calibration();
    }

    /// Request a full calibration and mark it as started for page state.
    fn begin_calibration(&mut self) {
        let _ = self.commands.send(ExperimentCommand::Calibrate);
        self.calibrate_started = true;
    }

    /// Space in the stimulus step: start, pause or resume.
    fn forward_stimuli(&mut self) {
        let (phase, calibrating) = {
            let console = lock_console(&self.shared);
            (console.phase, console.calibration.active)
        };
        if calibrating {
            // The automatic quick recalibration owns the presenter; wait for
            // it to finish or go back with Backspace.
            return;
        }
        let command = match phase {
            Phase::Idle => Some(ExperimentCommand::Start),
            Phase::Running => Some(ExperimentCommand::Pause),
            Phase::Paused => Some(ExperimentCommand::Resume),
            Phase::Finished => {
                self.enter(Step::Debrief);
                None
            }
            Phase::Aborted => {
                self.enter(Step::Debrief);
                None
            }
        };
        if let Some(command) = command {
            let _ = self.commands.send(command);
        }
    }

    /// Record that a calibration run actually started, for completion detection.
    fn sync_step(&mut self) {
        if self.step == Step::Calibrate && lock_console(&self.shared).calibration.active {
            self.calibration_seen_active = true;
        }
    }

    /// Move to the debrief automatically when the session finishes.
    fn advance_if_finished(&mut self) {
        if self.step == Step::Stimuli && lock_console(&self.shared).phase == Phase::Finished {
            self.enter(Step::Debrief);
        }
    }

    /// Close the presenter after the debrief page.
    fn close(&mut self) {
        self.close_requested = true;
        let _ = self.commands.send(ExperimentCommand::Shutdown);
    }

    /// Draw the current step page or status strip.
    fn draw_step(&self, ui: &mut egui::Ui) {
        match self.step {
            Step::Welcome => self.draw_page(
                ui,
                &self.intro,
                "Step 1 · Welcome · Space begin · F11 fullscreen",
            ),
            Step::Adjust => {}
            Step::Calibrate => self.draw_calibration(ui),
            Step::Stimuli => self.draw_stimulus_status(ui),
            Step::Debrief => self.draw_debrief(ui),
            Step::ConfirmCancel => self.draw_cancel_page(ui),
        }
    }

    /// Debrief page, augmented with an error if the session was stopped.
    fn draw_debrief(&self, ui: &mut egui::Ui) {
        let mut text = self.debrief.clone();
        if let Some(error) = lock_console(&self.shared).error.clone() {
            text.push_str(&format!("\n\n**Session stopped:** {error}"));
        }
        self.draw_page(ui, &text, "Step 5 · Debrief · Space close · F11 fullscreen");
    }

    /// Confirmation page reached by Backspace from the adjust step.
    fn draw_cancel_page(&self, ui: &mut egui::Ui) {
        let (completed, total) = {
            let console = lock_console(&self.shared);
            (console.trials_completed, console.trials_total)
        };
        let text = format!(
            "# Cancel the session?\n\nSure you want to cancel the session? **{}**.\n\n\
             Backspace exits and saves the result. Space returns to the camera adjustment.",
            saved_label(completed, total)
        );
        self.draw_page(
            ui,
            &text,
            "Backspace exits · Space back to adjust · F11 fullscreen",
        );
    }

    /// Full-page Markdown with a footer.
    fn draw_page(&self, ui: &mut egui::Ui, text: &str, footer: &str) {
        let rect = ui.max_rect();
        ui.painter()
            .rect_filled(rect, 0.0, egui::Color32::from_rgb(16, 16, 16));
        let scale = (rect.width() / 1500.0).clamp(0.8, 2.0);
        let width = (rect.width() * 0.78).min(1000.0 * scale);
        let content = egui::Rect::from_min_size(
            egui::pos2(
                rect.center().x - width * 0.5,
                rect.top() + rect.height() * 0.12,
            ),
            egui::vec2(width, rect.height() * 0.76),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
            show_markdown(ui, text, scale);
        });
        ui.painter().text(
            egui::pos2(rect.center().x, rect.bottom() - 22.0 * scale),
            egui::Align2::CENTER_BOTTOM,
            footer,
            egui::FontId::proportional(16.0 * scale),
            egui::Color32::from_gray(190),
        );
    }

    /// Calibration instructions or the live calibration strip.
    fn draw_calibration(&self, ui: &mut egui::Ui) {
        let (active, fitted, error, calibration_phase, engine_phase, report) = {
            let console = lock_console(&self.shared);
            (
                console.calibration.active,
                console.calibration.fitted,
                console.calibration.error.clone(),
                console.calibration.phase,
                console.phase,
                console.calibration.report,
            )
        };
        if active {
            self.draw_strip(
                ui,
                &format!(
                    "Calibration · {} · look at the orange target\nBackspace cancels · F11 fullscreen",
                    calibration_phase.label()
                ),
                None,
            );
            return;
        }

        let mut text = String::from("# Calibration\n\n");
        if let Some(report) = report {
            text.push_str(&format!(
                "Last run: **{}** samples · RMSE **{:.1} px**.\n\n",
                report.samples, report.rmse
            ));
        }
        if let Some(error) = &error {
            text.push_str(&format!("**Calibration failed:** {error}\n\n"));
            text.push_str("Press **Space** to retry, or **Backspace** to adjust the camera.");
        } else if self.calibration_seen_active && fitted {
            text.push_str(
                "Calibration complete. Press **Space** to start the stimuli. \
                 Backspace returns to the camera adjustment.",
            );
        } else if self.calibrate_started {
            text.push_str("Starting the calibration…");
        } else {
            text.push_str(
                "Press **Space** to run the full 3×3 calibration.\n\n\
                 Keep your head still and follow the orange targets.",
            );
        }
        if engine_phase == Phase::Paused {
            text.push_str("\n\nThe session is paused; calibration continues while paused.");
        }
        self.draw_page(
            ui,
            &text,
            "Step 3 · Calibration · Space continue · Backspace back · F11 fullscreen",
        );
    }

    /// A paused/starting stimulus shows a small strip; a running trial does not.
    fn draw_stimulus_status(&self, ui: &mut egui::Ui) {
        let (phase, error, calibrating, calibration_phase) = {
            let console = lock_console(&self.shared);
            (
                console.phase,
                console.error.clone(),
                console.calibration.active,
                console.calibration.phase,
            )
        };
        if calibrating {
            self.draw_strip(
                ui,
                &format!(
                    "Quick recalibration ({}) · look at the orange target\nBackspace returns to calibration · F11 fullscreen",
                    calibration_phase.label()
                ),
                None,
            );
            return;
        }
        if phase == Phase::Running && error.is_none() {
            return;
        }
        let line = match phase {
            Phase::Idle => {
                "Starting the stimuli…\nSpace pause · Backspace calibration · F11 fullscreen"
            }
            Phase::Paused => "Paused · Space resume · Backspace calibration · F11 fullscreen",
            Phase::Finished => return,
            Phase::Aborted => {
                "Session stopped · Space close · Backspace calibration · F11 fullscreen"
            }
            Phase::Running => "Running · Space pause · F11 fullscreen",
        };
        self.draw_strip(ui, line, error.as_deref());
    }

    /// Compact status box in the top-left corner.
    fn draw_strip(&self, ui: &mut egui::Ui, line: &str, error: Option<&str>) {
        let rect = ui.max_rect();
        let scale = (rect.width() / 1500.0).clamp(0.8, 2.0);
        let origin = rect.min + egui::vec2(18.0 * scale, 18.0 * scale);
        let width = (rect.width() - 36.0 * scale).min(820.0 * scale);
        let font = 16.0 * scale;
        // The step line plus every explicit line in `line`, plus an error line.
        let lines = line.lines().count() + 1 + usize::from(error.is_some());
        let height = lines as f32 * font * 1.3 + 18.0 * scale;
        let box_rect = egui::Rect::from_min_size(origin, egui::vec2(width, height));
        let painter = ui.painter();
        painter.rect_filled(box_rect, 6.0, egui::Color32::from_black_alpha(190));
        painter.text(
            origin + egui::vec2(12.0 * scale, 9.0 * scale),
            egui::Align2::LEFT_TOP,
            format!("Step {} · {}\n{line}", self.step.index(), self.step.label()),
            egui::FontId::proportional(font),
            egui::Color32::WHITE,
        );
        if let Some(error) = error {
            painter.text(
                origin + egui::vec2(12.0 * scale, height - 10.0 * scale),
                egui::Align2::LEFT_BOTTOM,
                error,
                egui::FontId::proportional(14.0 * scale),
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

    use super::{
        back_action, leaves_fullscreen, presenter_viewport, saved_label, start_step, BackAction,
        PresenterOptions, Step, DEFAULT_DEBRIEF, DEFAULT_INTRO,
    };

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

    #[test]
    fn the_linear_steps_are_numbered_and_named() {
        assert_eq!(Step::Welcome.index(), 1);
        assert_eq!(Step::Adjust.index(), 2);
        assert_eq!(Step::Calibrate.index(), 3);
        assert_eq!(Step::Stimuli.index(), 4);
        assert_eq!(Step::Debrief.index(), 5);
        assert_eq!(Step::Debrief.label(), "debrief");
        assert_eq!(Step::ConfirmCancel.label(), "cancel");
    }

    #[test]
    fn backspace_walks_stimuli_to_calibration_then_adjust_then_cancel() {
        let mut step = Step::Stimuli;
        let mut visited = Vec::new();
        loop {
            match back_action(step) {
                BackAction::Calibrate => {
                    visited.push(Step::Calibrate);
                    step = Step::Calibrate;
                }
                BackAction::Go(next) => {
                    visited.push(next);
                    step = next;
                }
                BackAction::Exit => break,
                BackAction::Stay => panic!("unexpected stay from {step:?}"),
            }
        }

        assert_eq!(
            visited,
            vec![Step::Calibrate, Step::Adjust, Step::ConfirmCancel]
        );
    }

    #[test]
    fn backspace_does_not_leave_the_outer_pages() {
        assert_eq!(back_action(Step::Welcome), BackAction::Stay);
        assert_eq!(back_action(Step::Debrief), BackAction::Stay);
    }

    #[test]
    fn escape_only_leaves_an_active_fullscreen() {
        assert!(leaves_fullscreen(Some(true)));
        assert!(!leaves_fullscreen(Some(false)));
        assert!(!leaves_fullscreen(None));
    }

    #[test]
    fn the_default_pages_are_present() {
        assert!(DEFAULT_INTRO.contains("Welcome"));
        assert!(DEFAULT_DEBRIEF.contains("You have completed the session"));
    }

    #[test]
    fn the_trial_saved_wording_shows_progress() {
        assert_eq!(saved_label(0, 18), "0/18 trials saved");
        assert_eq!(saved_label(2, 18), "2/18 trials saved");
    }

    #[test]
    fn adjust_starts_at_the_adjust_step() {
        let options = PresenterOptions::new(std::path::Path::new(".")).adjust(true);

        assert!(options.adjust);
        assert_eq!(start_step(options.adjust), Step::Adjust);
        assert_eq!(start_step(false), Step::Welcome);
    }
}

// SPDX-License-Identifier: GPL-3.0-only

//! Graphical experimenter console plus the stimulus window.
//!
//! One eframe event loop owns two viewports: the root viewport is the console,
//! and a deferred viewport on the configured monitor presents stimuli and
//! calibration grids. A dedicated controller thread owns the engine, the data
//! sink, the camera, and the presenter handle, so console repaints never drive
//! experiment time.
//!
//! Keyboard shortcuts: `F5` starts, `F6` pauses or resumes, `F7` runs a full
//! calibration, `F9` a quick recalibration, `F10` toggles the camera preview,
//! `Esc` cancels calibration, and `F8` aborts the session.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eframe::egui;
use yet_console::{
    ConsoleState, ConsoleUi, ExperimentCommand, LogLevel, MonitorChoice, PreviewFrame,
};
use yet_core::layout::StimulusPlacement;
use yet_core::{
    gaze_scene, step, CalibrationGrid, CalibrationPhase, CalibrationRun, CameraConfig,
    ConsoleEvent, Effect, EngineEvent, EngineState, FrameRecord, GazeMarkerStyle, GazePoint,
    GazeSample, Phase, QuadBrightness, QuickBackground, SessionPlan,
};
use yet_present::{OverlayBackend, Scene, ScreenGeometry, SessionTime};
use yet_present_egui::{EguiOverlay, EguiPresenter, PresenterConfig};

use crate::desktop_overlay::{self, OverlayHost};
use crate::driver::{gaze_sample, prepare, RunError, RunReport, SessionRunner};
use crate::tracker::Tracker;

/// Frame step of the real-time controller.
const TICK: Duration = Duration::from_micros(16_667);

/// How often the console repaints while a session is active.
const CONSOLE_REPAINT: Duration = Duration::from_millis(100);

/// How often the console repaints while the gaze overlay is visible.
const OVERLAY_REPAINT: Duration = Duration::from_millis(33);

/// Camera preview update interval (about 15 Hz).
const PREVIEW_INTERVAL: Duration = Duration::from_millis(66);

/// Warn when no camera frame has arrived for this long.
const CAMERA_STALE: Duration = Duration::from_secs(2);

/// Options for [`run_console`].
#[derive(Clone, Copy, Debug)]
pub struct ConsoleOptions<'a> {
    /// Experiment directory containing `config.toml` and the CSVs.
    pub experiment_dir: &'a Path,
    /// Participant identifier; defaults to the Unix time at session start.
    pub participant: Option<&'a str>,
    /// Result CSV path; defaults to the experiment directory.
    pub output: Option<&'a Path>,
    /// Override the configured camera index.
    pub device: Option<usize>,
}

impl<'a> ConsoleOptions<'a> {
    /// Create options for an experiment directory.
    pub fn new(experiment_dir: &'a Path) -> Self {
        Self {
            experiment_dir,
            participant: None,
            output: None,
            device: None,
        }
    }

    /// Override the participant identifier.
    pub fn participant(mut self, participant: &'a str) -> Self {
        self.participant = Some(participant);
        self
    }

    /// Override the result CSV path.
    pub fn output(mut self, output: &'a Path) -> Self {
        self.output = Some(output);
        self
    }

    /// Override the configured camera index.
    pub fn device(mut self, device: usize) -> Self {
        self.device = Some(device);
        self
    }
}

/// Open the console and run until the experimenter closes it.
pub fn run_console(options: ConsoleOptions<'_>) -> Result<(), RunError> {
    let prepared = prepare(options.experiment_dir, options.participant, options.output)?;
    let experiment_id = prepared.loaded.experiment.id.clone();
    let display = prepared.loaded.config.display.clone();
    let camera_config = prepared.loaded.config.camera.clone();
    let audio_config = prepared.loaded.config.audio.clone();
    let overlay = desktop_overlay::open(&prepared.loaded.config)?;
    let overlay_monitor = overlay.monitor;
    let overlay_notice = overlay.notice.clone();
    let overlay_available = overlay.host.is_available();
    let overlay_label = overlay.host.label();
    let (egui_overlay, thread_overlay) = match overlay.host {
        OverlayHost::Egui(overlay) => (Some(overlay), None),
        OverlayHost::Thread(overlay) => (None, Some(overlay)),
        OverlayHost::Off => (None, None),
    };
    let trials = prepared.loaded.plan.trial_count();
    let participant = prepared.sink.identity().participant.clone();

    let mut audio = yet_media::open_player(
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
        console.log(
            LogLevel::Info,
            format!(
                "experiment `{experiment_id}`, {trials} trials; result `{}`",
                prepared.result_file.display()
            ),
        );
        console.log(
            LogLevel::Info,
            format!(
                "presenter monitor {}, console monitor {}; renderer wgpu, AutoVsync, frame latency 1",
                display.presenter_monitor, display.console_monitor
            ),
        );
        console.log(
            LogLevel::Info,
            "keys: F5 start · F6 pause/resume · F7 calibrate · F9 quick · F10 preview · F8 abort",
        );
        console.log(LogLevel::Info, "waiting for the stimulus window…");
        console.overlay.available = overlay_available;
        console.overlay.enabled = overlay_available;
        console.overlay.error = overlay_notice.clone();
        if let Some(notice) = overlay_notice {
            console.log(LogLevel::Warning, notice);
        }
        if overlay_available {
            console.log(
                LogLevel::Info,
                format!("gaze overlay: {overlay_label} on monitor {overlay_monitor}"),
            );
        }
    }

    let (commands, controller) = spawn_controller(
        prepared.loaded.plan,
        prepared.sink,
        presenter.clone(),
        Arc::clone(&shared),
        camera_config,
        options.device,
        thread_overlay,
        overlay_monitor,
    )?;

    let app = ConsoleApp {
        shared,
        presenter,
        commands,
        controller: Some(controller),
        console: ConsoleUi::new(),
        overlay: egui_overlay,
        overlay_monitor,
        overlay_style: GazeMarkerStyle::default(),
        overlay_warned: false,
        logged_ready: false,
        sized: false,
    };

    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("yet-rs console · {experiment_id}"))
            .with_app_id("yet-rs-console")
            .with_inner_size([1400.0, 860.0])
            .with_min_inner_size([900.0, 620.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::WgpuConfiguration::default()
            .with_surface_config(eframe::SurfaceConfig::LOW_LATENCY),
        system_font_fallback: false,
        persist_window: false,
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "yet-rs console",
        native,
        Box::new(move |_cc| Ok(Box::new(app))),
    )
    .map_err(|error| RunError::Window(error.to_string()))
}

struct ConsoleApp {
    shared: Arc<Mutex<ConsoleState>>,
    presenter: EguiPresenter,
    commands: Sender<ExperimentCommand>,
    controller: Option<JoinHandle<()>>,
    console: ConsoleUi,
    overlay: Option<EguiOverlay>,
    overlay_monitor: usize,
    overlay_style: GazeMarkerStyle,
    overlay_warned: bool,
    logged_ready: bool,
    sized: bool,
}

impl eframe::App for ConsoleApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // The overlay viewport uses this clear color; the console paints its own
        // opaque background below.
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, egui::Color32::from_rgb(12, 12, 12));
        if !self.sized {
            if let Some(monitor) = ctx.input(|input| input.viewport().monitor_size) {
                let target = egui::vec2(1400.0, 860.0).min(monitor * 0.92);
                let origin = ((monitor - target) * 0.5).max(egui::Vec2::ZERO);
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target));
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                    origin.x, origin.y,
                )));
                self.sized = true;
            }
        }
        let status = self.presenter.status();
        let monitors = monitor_choices(frame);

        if !monitors.is_empty() {
            let mut console = lock_console(&self.shared);
            if let Some(message) = console.adopt_monitors(monitors.len()) {
                console.log(LogLevel::Warning, message);
            }
        }

        if status.ready && !self.logged_ready {
            self.logged_ready = true;
            let mut console = lock_console(&self.shared);
            console.log(
                LogLevel::Info,
                format!(
                    "stimulus window ready: {}×{} px at {:.3} px/pt",
                    status.size_px.0, status.size_px.1, status.scale_factor
                ),
            );
        }

        let requested = {
            let mut console = lock_console(&self.shared);
            let presenter = yet_console::PresenterStatus {
                ready: status.ready,
                monitor: status.monitor,
                fullscreen: status.fullscreen,
                size_px: status.size_px,
                scale_factor: status.scale_factor,
                paints: status.paints,
                presents: status.presents,
                coalesced: status.coalesced,
                paint_hz: status.paint_hz,
                last_dt: status.last_dt,
                media_count: status.media_count,
                focused: status.focused,
            };
            self.console.ui(ui, &mut console, presenter, &monitors)
        };
        for command in requested {
            let focus_presenter = matches!(
                command,
                ExperimentCommand::Start
                    | ExperimentCommand::Resume
                    | ExperimentCommand::Calibrate
                    | ExperimentCommand::QuickCalibrate
            );
            let _ = self.commands.send(command);
            if focus_presenter {
                // KWin's "Dim Inactive" effect darkens an unfocused window by
                // 25%, which would change the stimulus luminance. Ask the
                // stimulus viewport itself to take focus.
                self.presenter.request_focus();
            }
        }

        let (monitor, fullscreen) = {
            let console = lock_console(&self.shared);
            (console.presenter_monitor, console.fullscreen)
        };
        if monitor != status.monitor || fullscreen != status.fullscreen {
            self.presenter.set_monitor(monitor, fullscreen);
            let mut console = lock_console(&self.shared);
            console.log(
                LogLevel::Info,
                format!("presenter monitor {monitor}, fullscreen {fullscreen}"),
            );
        }

        yet_present_egui::show_viewport(&ctx, &self.presenter.shared());
        self.update_overlay(&ctx, &status);
        let repaint = if self.overlay.as_ref().is_some_and(EguiOverlay::is_visible) {
            OVERLAY_REPAINT
        } else {
            CONSOLE_REPAINT
        };
        ctx.request_repaint_after(repaint);
    }
}

impl ConsoleApp {
    fn update_overlay(&mut self, ctx: &egui::Context, status: &yet_present_egui::PresenterStatus) {
        let Some(overlay) = &mut self.overlay else {
            return;
        };
        let (enabled, gaze, presenter_monitor, fullscreen, phase) = {
            let console = lock_console(&self.shared);
            (
                console.overlay.enabled,
                console.overlay.gaze,
                console.presenter_monitor,
                console.fullscreen,
                console.phase,
            )
        };

        if enabled && presenter_monitor != self.overlay_monitor && !self.overlay_warned {
            self.overlay_warned = true;
            let mut console = lock_console(&self.shared);
            let message = format!(
                "overlay is bound to monitor {}; restart to move it",
                self.overlay_monitor
            );
            console.overlay.error = Some(message.clone());
            console.log(LogLevel::Warning, message);
        }

        let usable = enabled
            && presenter_monitor == self.overlay_monitor
            && fullscreen
            && status.ready
            && phase == Phase::Running;
        if !usable {
            overlay.set_visible(false);
            overlay.show_viewport(ctx);
            return;
        }

        let geometry = ScreenGeometry::new(
            self.overlay_monitor,
            None,
            status.size_px,
            status.scale_factor,
        );
        overlay.set_scene(gaze_scene(geometry, geometry, gaze, &self.overlay_style));
        if let Err(error) = overlay.show(self.overlay_monitor) {
            let mut console = lock_console(&self.shared);
            console.overlay.available = false;
            console.overlay.error = Some(error.to_string());
            console.overlay.enabled = false;
            console.log(LogLevel::Warning, format!("overlay stopped: {error}"));
        }
        overlay.show_viewport(ctx);
    }
}

impl Drop for ConsoleApp {
    fn drop(&mut self) {
        let _ = self.commands.send(ExperimentCommand::Shutdown);
        if let Some(controller) = self.controller.take() {
            let _ = controller.join();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_controller(
    plan: SessionPlan,
    mut sink: yet_core::DataSink<std::io::BufWriter<std::fs::File>>,
    mut presenter: EguiPresenter,
    shared: Arc<Mutex<ConsoleState>>,
    camera_config: CameraConfig,
    device: Option<usize>,
    overlay: Option<Box<dyn OverlayBackend + Send>>,
    overlay_monitor: usize,
) -> Result<(Sender<ExperimentCommand>, JoinHandle<()>), RunError> {
    let (commands, receiver) = mpsc::channel();
    let handle = thread::Builder::new()
        .name("yet-session".to_owned())
        .spawn(move || {
            let trials = plan.trial_count();
            let mut state = EngineState::new(plan);
            let latest_gaze: Rc<Cell<Option<GazePoint>>> = Rc::new(Cell::new(None));
            let gaze_cell = Rc::clone(&latest_gaze);
            let mut runner = SessionRunner::new(
                &mut sink,
                &mut presenter,
                move |record: &FrameRecord, placement: Option<StimulusPlacement>| {
                    let monitor = gaze_cell.take()?;
                    let placement = placement?;
                    Some(gaze_sample(record, monitor, placement))
                },
            );
            runner.report_mut().trials = trials;
            let start = Instant::now();
            run_loop(
                start,
                &mut state,
                &mut runner,
                &receiver,
                &shared,
                &latest_gaze,
                &camera_config,
                device,
                overlay,
                overlay_monitor,
            );

            if let Err(error) = runner.flush() {
                let mut console = lock_console(&shared);
                console.error = Some(error.to_string());
                console.log(LogLevel::Error, error.to_string());
            }
            drop(runner);

            let mut console = lock_console(&shared);
            console.finished = true;
            console.log(LogLevel::Info, "controller stopped");
        })
        .map_err(|error| RunError::Thread(error.to_string()))?;
    Ok((commands, handle))
}

#[allow(clippy::too_many_arguments)]
fn run_loop<G>(
    start: Instant,
    state: &mut EngineState,
    runner: &mut SessionRunner<'_, std::io::BufWriter<std::fs::File>, EguiPresenter, G>,
    commands: &Receiver<ExperimentCommand>,
    shared: &Arc<Mutex<ConsoleState>>,
    latest_gaze: &Rc<Cell<Option<GazePoint>>>,
    camera_config: &CameraConfig,
    device: Option<usize>,
    mut overlay: Option<Box<dyn OverlayBackend + Send>>,
    overlay_monitor: usize,
) where
    G: FnMut(&FrameRecord, Option<StimulusPlacement>) -> Option<GazeSample>,
{
    let mut next_tick = Instant::now();
    let mut last_tick = next_tick;
    let mut shut_down = false;
    let mut tracker: Option<Tracker> = None;
    let mut calibration: Option<CalibrationRun> = None;
    let mut last_calibration_phase = CalibrationPhase::Idle;
    let mut last_calibration_target = None;
    let mut preview_generation = 0_u64;
    let mut last_preview: Option<Instant> = None;
    let mut first_trial = false;
    let overlay_style = GazeMarkerStyle::default();
    let mut overlay_visible = false;
    let mut overlay_gaze: Option<GazePoint> = None;
    let mut overlay_warned = false;

    while !shut_down {
        let now = Instant::now();
        {
            let mut console = lock_console(shared);
            console.log_elapsed = now.duration_since(start);
        }
        sync_overlay(
            state,
            overlay.as_mut(),
            runner.geometry(),
            shared,
            latest_gaze.get(),
            &mut overlay_visible,
            &mut overlay_gaze,
            &overlay_style,
            overlay_monitor,
            &mut overlay_warned,
        );

        // Console intent: preview on/off and a retry request.
        let (preview_enabled, retry) = {
            let console = lock_console(shared);
            (console.preview.enabled, console.preview.retry)
        };
        if retry {
            let mut console = lock_console(shared);
            console.preview.retry = false;
            console.preview.error = None;
            drop(console);
            tracker = None;
        }
        if preview_enabled && tracker.is_none() {
            tracker = open_tracker(camera_config, device, shared);
        }

        let calibrating = calibration.as_ref().is_some_and(CalibrationRun::is_active);
        let running = state.phase == Phase::Running;
        let fitted = calibration.as_ref().is_some_and(CalibrationRun::is_fitted);
        let want_features = calibrating || preview_enabled || (running && fitted);
        let mut features: Option<QuadBrightness> = None;

        if want_features {
            if let Some(tracker) = tracker.as_mut() {
                if let Some(polled) = tracker.poll() {
                    features = Some(polled.features);
                    if running && fitted {
                        if let Some(run) = &calibration {
                            if let Ok(point) = run.predict(polled.features) {
                                latest_gaze.set(Some(point));
                                lock_console(shared).overlay.gaze = Some(point);
                            }
                        }
                    }
                    if preview_enabled
                        && last_preview
                            .is_none_or(|last| now.duration_since(last) >= PREVIEW_INTERVAL)
                    {
                        update_preview(
                            shared,
                            tracker,
                            polled,
                            calibration.as_ref(),
                            runner.geometry().size_px,
                            &mut preview_generation,
                        );
                        last_preview = Some(now);
                    }
                    if preview_enabled {
                        let mut console = lock_console(shared);
                        console.preview.error = None;
                        console.preview.capture_fps = tracker.capture_fps();
                    }
                } else if preview_enabled {
                    if let Some(age) = tracker.last_frame_age(now) {
                        if age > CAMERA_STALE {
                            let mut console = lock_console(shared);
                            if console.preview.error.is_none() {
                                console.preview.error =
                                    Some(format!("no frames for {:.0} s", age.as_secs_f32()));
                            }
                        }
                    }
                }
            }
        }

        // Wait for a command or the next tick.
        let active = running || calibrating;
        let wait = if active {
            next_tick.saturating_duration_since(now)
        } else {
            Duration::from_millis(50)
        }
        .max(Duration::from_millis(1));

        match commands.recv_timeout(wait) {
            Ok(ExperimentCommand::Shutdown) => {
                cancel_calibration(state, runner, calibration.as_mut(), shared, start);
                abort_if_active(start, state, runner, shared);
                shut_down = true;
            }
            Ok(command) => {
                let at = session_time(start, Instant::now());
                match command {
                    ExperimentCommand::Shutdown => unreachable!(),
                    ExperimentCommand::Start => {
                        if calibration.as_ref().is_some_and(CalibrationRun::is_active) {
                            log_warning(shared, "finish or cancel the calibration first");
                        } else {
                            if calibration.as_ref().is_some_and(CalibrationRun::is_fitted)
                                && tracker.is_none()
                            {
                                tracker = open_tracker(camera_config, device, shared);
                            }
                            if !calibration.as_ref().is_some_and(CalibrationRun::is_fitted) {
                                log_warning(
                                    shared,
                                    "starting without calibration; no gaze rows will be written",
                                );
                            }
                            let effects = step(state, EngineEvent::Start, at);
                            clear_gaze(shared, latest_gaze);
                            if let Err(error) = apply_effects(
                                state,
                                runner,
                                effects,
                                shared,
                                at,
                                start,
                                &mut first_trial,
                            ) {
                                fail(state, runner, shared, error);
                            }
                            last_tick = Instant::now();
                            next_tick = last_tick;
                        }
                    }
                    ExperimentCommand::Pause | ExperimentCommand::Resume => {
                        if calibrating {
                            log_warning(shared, "finish or cancel the calibration first");
                        } else {
                            let event = if command == ExperimentCommand::Pause {
                                EngineEvent::Pause
                            } else {
                                EngineEvent::Resume
                            };
                            if event == EngineEvent::Pause {
                                clear_gaze(shared, latest_gaze);
                            }
                            let effects = step(state, event, at);
                            if let Err(error) = apply_effects(
                                state,
                                runner,
                                effects,
                                shared,
                                at,
                                start,
                                &mut first_trial,
                            ) {
                                fail(state, runner, shared, error);
                            }
                            last_tick = Instant::now();
                            next_tick = last_tick;
                        }
                    }
                    ExperimentCommand::Abort => {
                        if calibrating {
                            cancel_calibration(state, runner, calibration.as_mut(), shared, start);
                        }
                        if matches!(state.phase, Phase::Running | Phase::Paused) {
                            let effects = step(state, EngineEvent::Abort, at);
                            clear_gaze(shared, latest_gaze);
                            if let Err(error) = apply_effects(
                                state,
                                runner,
                                effects,
                                shared,
                                at,
                                start,
                                &mut first_trial,
                            ) {
                                fail(state, runner, shared, error);
                            }
                        }
                        last_tick = Instant::now();
                        next_tick = last_tick;
                    }
                    ExperimentCommand::Calibrate => {
                        if calibrating {
                            log_warning(shared, "a calibration is already running");
                        } else if !matches!(state.phase, Phase::Idle | Phase::Finished) {
                            log_warning(
                                shared,
                                "a full calibration is available when the session is idle",
                            );
                        } else {
                            start_full_calibration(
                                start,
                                runner,
                                &mut calibration,
                                &mut tracker,
                                camera_config,
                                device,
                                shared,
                                &mut last_calibration_phase,
                                &mut last_calibration_target,
                            );
                            last_tick = Instant::now();
                            next_tick = last_tick;
                        }
                    }
                    ExperimentCommand::QuickCalibrate => {
                        if calibrating {
                            log_warning(shared, "a calibration is already running");
                        } else if !matches!(
                            state.phase,
                            Phase::Idle | Phase::Paused | Phase::Finished
                        ) {
                            log_warning(shared, "pause the session before a quick recalibration");
                        } else {
                            start_quick_calibration(
                                start,
                                state,
                                runner,
                                &mut calibration,
                                &mut tracker,
                                camera_config,
                                device,
                                shared,
                                &mut last_calibration_phase,
                                &mut last_calibration_target,
                            );
                            last_tick = Instant::now();
                            next_tick = last_tick;
                        }
                    }
                    ExperimentCommand::CancelCalibration => {
                        cancel_calibration(state, runner, calibration.as_mut(), shared, start);
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                cancel_calibration(state, runner, calibration.as_mut(), shared, start);
                abort_if_active(start, state, runner, shared);
                shut_down = true;
            }
        }

        if shut_down {
            break;
        }

        let now = Instant::now();
        let calibrating = calibration.as_ref().is_some_and(CalibrationRun::is_active);

        if calibrating {
            if now < next_tick {
                continue;
            }
            let at = session_time(start, now);
            let dt = now.duration_since(last_tick).max(Duration::from_micros(1));
            last_tick = now;

            let (result, scene) = {
                let run = calibration.as_mut().expect("calibration is active");
                let result = run.tick(at, features);
                (result, run.scene())
            };
            runner.present_scene(at, dt, &scene);
            if let Some(run) = &calibration {
                sync_calibration(shared, run, start);
                log_calibration_transition(
                    shared,
                    run,
                    &mut last_calibration_phase,
                    &mut last_calibration_target,
                );
            }

            let failed = result.is_err();
            if failed {
                // Force a fresh camera on the next attempt; this also recovers
                // from a device that stopped delivering frames.
                tracker = None;
            }
            let finished = calibration.as_ref().is_none_or(|run| !run.is_active());
            let phase = calibration.as_ref().map(CalibrationRun::phase);
            if failed {
                finish_calibration_view(state, runner, at);
            } else if finished {
                if phase == Some(CalibrationPhase::Done) {
                    if state.phase == Phase::Paused {
                        runner.present(at, TICK);
                    }
                } else {
                    finish_calibration_view(state, runner, at);
                }
            }

            next_tick += TICK;
            if next_tick <= now {
                next_tick = now + TICK;
            }
            continue;
        }

        if state.phase != Phase::Running || now < next_tick {
            continue;
        }

        let at = session_time(start, now);
        let dt = now.duration_since(last_tick).max(Duration::from_micros(1));
        last_tick = now;
        let effects = step(state, EngineEvent::Tick, at);
        if let Err(error) =
            apply_effects(state, runner, effects, shared, at, start, &mut first_trial)
        {
            fail(state, runner, shared, error);
            continue;
        }
        if state.phase == Phase::Running {
            let experiment_t = SessionTime::from_duration(state.elapsed(at).unwrap_or_default());
            let stats = runner.present(experiment_t, dt);
            let mut console = lock_console(shared);
            if let Some(message) = console.record_present(
                stats.dropped_frames,
                stats.audio_drift_ms,
                stats.audio_underruns,
                stats.frame.media_pts,
            ) {
                console.log(LogLevel::Warning, message);
            }
        }

        next_tick += TICK;
        if next_tick <= now {
            next_tick = now + TICK;
        }
    }
}

fn clear_gaze(shared: &Arc<Mutex<ConsoleState>>, latest_gaze: &Rc<Cell<Option<GazePoint>>>) {
    latest_gaze.set(None);
    lock_console(shared).overlay.gaze = None;
}

#[allow(clippy::too_many_arguments)]
fn sync_overlay(
    state: &EngineState,
    overlay: Option<&mut Box<dyn OverlayBackend + Send>>,
    geometry: ScreenGeometry,
    shared: &Arc<Mutex<ConsoleState>>,
    gaze: Option<GazePoint>,
    visible: &mut bool,
    last_gaze: &mut Option<GazePoint>,
    style: &GazeMarkerStyle,
    overlay_monitor: usize,
    warned: &mut bool,
) {
    let Some(overlay) = overlay else {
        return;
    };
    let (enabled, presenter_monitor) = {
        let console = lock_console(shared);
        (console.overlay.enabled, console.presenter_monitor)
    };

    if enabled && presenter_monitor != overlay_monitor && !*warned {
        *warned = true;
        let mut console = lock_console(shared);
        let message = format!("overlay is bound to monitor {overlay_monitor}; restart to move it");
        console.overlay.error = Some(message.clone());
        console.log(LogLevel::Warning, message);
    }

    let show = enabled && presenter_monitor == overlay_monitor && state.phase == Phase::Running;
    if show != *visible {
        if show {
            let _ = overlay.show(overlay_monitor);
            let clear = gaze_scene(geometry, geometry, None, style);
            let _ = overlay.present(&clear);
        } else {
            overlay.set_visible(false);
        }
        *visible = show;
        *last_gaze = None;
    }
    if !show || gaze == *last_gaze {
        return;
    }

    let scene = gaze_scene(geometry, geometry, gaze, style);
    if let Err(error) = overlay.present(&scene) {
        let mut console = lock_console(shared);
        console.overlay.available = false;
        console.overlay.error = Some(error.to_string());
        console.overlay.enabled = false;
        console.log(LogLevel::Warning, format!("overlay stopped: {error}"));
        *visible = false;
    }
    *last_gaze = gaze;
}

#[allow(clippy::too_many_arguments)]
fn start_full_calibration<G>(
    start: Instant,
    runner: &SessionRunner<'_, std::io::BufWriter<std::fs::File>, EguiPresenter, G>,
    calibration: &mut Option<CalibrationRun>,
    tracker: &mut Option<Tracker>,
    camera_config: &CameraConfig,
    device: Option<usize>,
    shared: &Arc<Mutex<ConsoleState>>,
    last_phase: &mut CalibrationPhase,
    last_target: &mut Option<usize>,
) where
    G: FnMut(&FrameRecord, Option<StimulusPlacement>) -> Option<GazeSample>,
{
    if tracker.is_none() {
        *tracker = open_tracker(camera_config, device, shared);
    }
    if tracker.is_none() {
        let mut console = lock_console(shared);
        console.calibration.error = console.preview.error.clone();
        return;
    }

    let surface = runner.geometry().size_px;
    let mut run = match calibration.take() {
        Some(run) if run.grid().surface_px() == surface => run,
        _ => CalibrationRun::new(CalibrationGrid::new(surface)),
    };
    let at = session_time(start, Instant::now());
    match run.start(at) {
        Ok(()) => {
            {
                let mut console = lock_console(shared);
                console.calibration.error = None;
                console.log(
                    LogLevel::Info,
                    format!(
                        "full calibration: {} targets on {}×{}; look at the orange target",
                        run.grid().len(),
                        surface.0,
                        surface.1
                    ),
                );
            }
            *last_phase = CalibrationPhase::Idle;
            *last_target = None;
            *calibration = Some(run);
            if let Some(run) = &*calibration {
                sync_calibration(shared, run, start);
                log_calibration_transition(shared, run, last_phase, last_target);
            }
        }
        Err(error) => log_error(shared, format!("calibration: {error}")),
    }
}

#[allow(clippy::too_many_arguments)]
fn start_quick_calibration<G>(
    start: Instant,
    state: &EngineState,
    runner: &SessionRunner<'_, std::io::BufWriter<std::fs::File>, EguiPresenter, G>,
    calibration: &mut Option<CalibrationRun>,
    tracker: &mut Option<Tracker>,
    camera_config: &CameraConfig,
    device: Option<usize>,
    shared: &Arc<Mutex<ConsoleState>>,
    last_phase: &mut CalibrationPhase,
    last_target: &mut Option<usize>,
) where
    G: FnMut(&FrameRecord, Option<StimulusPlacement>) -> Option<GazeSample>,
{
    let Some(run) = calibration.as_mut() else {
        log_warning(
            shared,
            "run a full calibration before a quick recalibration",
        );
        return;
    };
    if !run.is_fitted() {
        log_warning(
            shared,
            "run a full calibration before a quick recalibration",
        );
        return;
    }
    let surface = runner.geometry().size_px;
    if run.grid().surface_px() != surface {
        log_error(
            shared,
            "the presenter surface changed; run a full calibration first",
        );
        return;
    }
    if tracker.is_none() {
        *tracker = open_tracker(camera_config, device, shared);
    }
    if tracker.is_none() {
        let mut console = lock_console(shared);
        console.calibration.error = console.preview.error.clone();
        return;
    }

    let background = match (runner.current_media(), runner.placement()) {
        (Some(media), Some(placement)) => {
            let radius =
                (surface.0.min(surface.1) as f32 / 4.0 / placement.scale.max(f32::EPSILON))
                    .max(1.0);
            Some(QuickBackground {
                media,
                dest_px: placement.dest_px,
                radius_px: radius,
            })
        }
        _ => None,
    };
    let at = session_time(start, Instant::now());
    run.set_quick_background(background);
    match run.quick(at) {
        Ok(()) => {
            {
                let mut console = lock_console(shared);
                console.calibration.error = None;
                console.log(
                    LogLevel::Info,
                    "quick recalibration: look at the orange center target",
                );
                if state.phase == Phase::Paused {
                    console.log(
                        LogLevel::Info,
                        "the session is paused; it stays paused after recalibration",
                    );
                }
            }
            *last_phase = CalibrationPhase::Idle;
            *last_target = None;
            sync_calibration(shared, run, start);
            log_calibration_transition(shared, run, last_phase, last_target);
        }
        Err(error) => log_error(shared, format!("quick recalibration: {error}")),
    }
}

fn cancel_calibration<G>(
    state: &EngineState,
    runner: &mut SessionRunner<'_, std::io::BufWriter<std::fs::File>, EguiPresenter, G>,
    calibration: Option<&mut CalibrationRun>,
    shared: &Arc<Mutex<ConsoleState>>,
    start: Instant,
) -> bool
where
    G: FnMut(&FrameRecord, Option<StimulusPlacement>) -> Option<GazeSample>,
{
    let Some(run) = calibration else {
        return false;
    };
    if !run.is_active() {
        return false;
    }
    run.cancel();
    sync_calibration(shared, run, start);
    {
        let mut console = lock_console(shared);
        console.log(LogLevel::Info, "calibration cancelled");
    }
    let at = session_time(start, Instant::now());
    finish_calibration_view(state, runner, at);
    true
}

fn finish_calibration_view<G>(
    state: &EngineState,
    runner: &mut SessionRunner<'_, std::io::BufWriter<std::fs::File>, EguiPresenter, G>,
    at: SessionTime,
) where
    G: FnMut(&FrameRecord, Option<StimulusPlacement>) -> Option<GazeSample>,
{
    if state.phase == Phase::Paused {
        runner.present(at, TICK);
    } else {
        runner.present_scene(at, TICK, &Scene::new());
    }
}

fn sync_calibration(shared: &Arc<Mutex<ConsoleState>>, run: &CalibrationRun, start: Instant) {
    let snapshot = run.snapshot();
    let mut console = lock_console(shared);
    console.apply_calibration(snapshot);
    if let Some(error) = run.error() {
        console.calibration.error = Some(error.to_owned());
    }
    console.log_elapsed = start.elapsed();
}

fn log_calibration_transition(
    shared: &Arc<Mutex<ConsoleState>>,
    run: &CalibrationRun,
    last_phase: &mut CalibrationPhase,
    last_target: &mut Option<usize>,
) {
    let snapshot = run.snapshot();
    let mut console = lock_console(shared);

    if snapshot.target != *last_target && matches!(snapshot.phase, CalibrationPhase::Settle) {
        if let Some(target) = snapshot.target {
            console.log(
                LogLevel::Info,
                format!("calibration target {}/{}", target + 1, snapshot.total),
            );
        }
        *last_target = snapshot.target;
    }

    if snapshot.phase != *last_phase {
        match snapshot.phase {
            CalibrationPhase::QuickSettle => {
                console.log(LogLevel::Info, "quick recalibration: center target");
            }
            CalibrationPhase::FullReport => {
                if let Some(report) = snapshot.report {
                    console.log(
                        LogLevel::Info,
                        format!(
                            "calibration: {} samples, RMSE {:.1} px",
                            report.samples, report.rmse
                        ),
                    );
                }
            }
            CalibrationPhase::QuickReport => {
                if let Some(report) = snapshot.quick_report {
                    let drift = console
                        .calibration
                        .drift_rmse
                        .map(|drift| format!(", drift {drift:+.1} px"))
                        .unwrap_or_default();
                    console.log(
                        LogLevel::Info,
                        format!(
                            "quick recalibration: {} samples, RMSE {:.1} px{drift}",
                            report.samples, report.rmse
                        ),
                    );
                }
            }
            CalibrationPhase::Done => {
                console.log(LogLevel::Info, "calibration complete");
            }
            CalibrationPhase::Failed => {
                if let Some(error) = run.error() {
                    console.log(LogLevel::Error, format!("calibration failed: {error}"));
                }
            }
            CalibrationPhase::Cancelled => {
                console.log(LogLevel::Info, "calibration cancelled");
            }
            CalibrationPhase::Idle
            | CalibrationPhase::Settle
            | CalibrationPhase::Collect
            | CalibrationPhase::QuickCollect => {}
        }
        *last_phase = snapshot.phase;
    }
}

fn update_preview(
    shared: &Arc<Mutex<ConsoleState>>,
    tracker: &Tracker,
    polled: crate::tracker::PolledFrame,
    run: Option<&CalibrationRun>,
    surface_px: (u32, u32),
    generation: &mut u64,
) {
    let Some((width, height, data)) = tracker.preview(&polled.frame) else {
        return;
    };
    let gaze = run.and_then(|run| {
        run.is_fitted()
            .then(|| run.predict(polled.features).ok())
            .flatten()
    });
    let gaze_normalized = gaze.map(|gaze| {
        GazePoint::new(
            gaze.x / surface_px.0.max(1) as f32,
            gaze.y / surface_px.1.max(1) as f32,
        )
    });
    *generation += 1;
    let frame = PreviewFrame {
        width,
        height,
        data: Arc::from(data),
        generation: *generation,
        gaze,
        gaze_normalized,
    };
    let mut console = lock_console(shared);
    console.preview.frame = Some(frame);
    console.preview.error = None;
    console.preview.capture_fps = tracker.capture_fps();
}

fn open_tracker(
    config: &CameraConfig,
    device: Option<usize>,
    shared: &Arc<Mutex<ConsoleState>>,
) -> Option<Tracker> {
    match Tracker::open(config, device) {
        Ok(tracker) => {
            let info = tracker.info().clone();
            let roi = tracker.roi();
            {
                let mut console = lock_console(shared);
                console.preview.error = None;
                console.log(
                    LogLevel::Info,
                    format!(
                        "camera: {} ({}) {}×{} {}; eye ROI x={} y={} {}×{}",
                        info.name,
                        info.device,
                        info.width,
                        info.height,
                        info.format,
                        roi.x,
                        roi.y,
                        roi.width,
                        roi.height
                    ),
                );
            }
            Some(tracker)
        }
        Err(error) => {
            let message = error.to_string();
            let mut console = lock_console(shared);
            console.preview.error = Some(message.clone());
            if console.calibration.active {
                console.calibration.error = Some(message.clone());
            }
            console.log(LogLevel::Error, format!("camera: {message}"));
            None
        }
    }
}

fn abort_if_active<G>(
    start: Instant,
    state: &mut EngineState,
    runner: &mut SessionRunner<'_, std::io::BufWriter<std::fs::File>, EguiPresenter, G>,
    shared: &Arc<Mutex<ConsoleState>>,
) where
    G: FnMut(&FrameRecord, Option<StimulusPlacement>) -> Option<GazeSample>,
{
    if matches!(state.phase, Phase::Running | Phase::Paused) {
        let at = session_time(start, Instant::now());
        let effects = step(state, EngineEvent::Abort, at);
        let mut first_trial = false;
        if let Err(error) =
            apply_effects(state, runner, effects, shared, at, start, &mut first_trial)
        {
            fail(state, runner, shared, error);
        }
    }
}

fn fail<G>(
    state: &mut EngineState,
    runner: &mut SessionRunner<'_, std::io::BufWriter<std::fs::File>, EguiPresenter, G>,
    shared: &Arc<Mutex<ConsoleState>>,
    error: RunError,
) where
    G: FnMut(&FrameRecord, Option<StimulusPlacement>) -> Option<GazeSample>,
{
    {
        let mut console = lock_console(shared);
        console.error = Some(error.to_string());
        console.log(LogLevel::Error, error.to_string());
    }

    if matches!(state.phase, Phase::Running | Phase::Paused) {
        let at = state.last_tick.unwrap_or(SessionTime::ZERO);
        let effects = step(state, EngineEvent::Abort, at);
        for effect in effects {
            record_effect(shared, &effect);
            if let Err(follow_up) = runner.apply(effect) {
                let mut console = lock_console(shared);
                console.log(LogLevel::Error, follow_up.to_string());
            }
        }
    }
}

fn apply_effects<G>(
    state: &mut EngineState,
    runner: &mut SessionRunner<'_, std::io::BufWriter<std::fs::File>, EguiPresenter, G>,
    effects: Vec<Effect>,
    shared: &Arc<Mutex<ConsoleState>>,
    at: SessionTime,
    start: Instant,
    first_trial: &mut bool,
) -> Result<(), RunError>
where
    G: FnMut(&FrameRecord, Option<StimulusPlacement>) -> Option<GazeSample>,
{
    update_progress(shared, runner.report(), state, at);
    for effect in effects {
        record_effect(shared, &effect);
        if !*first_trial && matches!(&effect, Effect::TrialStarted { .. }) {
            *first_trial = true;
            let seconds = start.elapsed().as_secs_f32();
            let mut console = lock_console(shared);
            console.first_trial_seconds = Some(seconds);
            console.log(LogLevel::Info, format!("first trial after {seconds:.1} s"));
        }
        runner.apply(effect)?;
        update_progress(shared, runner.report(), state, at);
    }
    Ok(())
}

fn record_effect(shared: &Arc<Mutex<ConsoleState>>, effect: &Effect) {
    let mut console = lock_console(shared);
    match effect {
        Effect::TrialStarted {
            block,
            trial_index,
            stimulus,
        } => {
            console.current_block = Some(block.clone());
            console.current_trial = Some(*trial_index);
            console.current_stimulus = Some(stimulus.id.clone());
            console.log(
                LogLevel::Info,
                format!("block {block}, trial {}: {}", trial_index + 1, stimulus.id),
            );
        }
        Effect::AppendTrial(record) => {
            if record.completed {
                let timing = if record.late_by.is_zero() {
                    "on time".to_owned()
                } else {
                    format!("late {:.1} ms", record.late_by.as_secs_f64() * 1000.0)
                };
                console.log(
                    LogLevel::Info,
                    format!("trial {} finished ({timing})", record.stimulus),
                );
            }
        }
        Effect::NotifyConsole(event) => match event {
            ConsoleEvent::PhaseChanged(phase) => {
                console.phase = *phase;
                console.log(LogLevel::Info, format!("phase: {}", phase_label(*phase)));
            }
            ConsoleEvent::Warning(message) => console.log(LogLevel::Warning, message),
            ConsoleEvent::Tick { .. }
            | ConsoleEvent::Key(_)
            | ConsoleEvent::CalibrationSample(_)
            | ConsoleEvent::MediaReady(_) => {}
        },
        Effect::Error(error) => {
            console.error = Some(error.to_string());
            console.log(LogLevel::Error, error.to_string());
        }
        Effect::Present(_) | Effect::Presenter(_) | Effect::AppendFrame(_) => {}
    }
}

fn update_progress(
    shared: &Arc<Mutex<ConsoleState>>,
    report: &RunReport,
    state: &EngineState,
    at: SessionTime,
) {
    let mut console = lock_console(shared);
    console.phase = state.phase;
    console.trials_completed = report.completed;
    console.late_trials = report.late_trials;
    console.max_late = report.max_late;
    console.samples = report.samples;
    if let Some(elapsed) = state.elapsed(at) {
        console.elapsed = elapsed;
    }
}

fn phase_label(phase: Phase) -> &'static str {
    match phase {
        Phase::Idle => "idle",
        Phase::Running => "running",
        Phase::Paused => "paused",
        Phase::Finished => "finished",
        Phase::Aborted => "aborted",
    }
}

fn session_time(start: Instant, now: Instant) -> SessionTime {
    SessionTime::from_duration(now.saturating_duration_since(start))
}

fn plan_has_audio(plan: &SessionPlan) -> bool {
    plan.blocks
        .iter()
        .flat_map(|block| &block.trials)
        .any(|planned| planned.trial.stimulus.audio.is_some())
}

fn monitor_choices(frame: &eframe::Frame) -> Vec<MonitorChoice> {
    let Some(window) = frame.winit_window() else {
        return Vec::new();
    };
    window
        .available_monitors()
        .enumerate()
        .map(|(index, monitor)| {
            let size = monitor.size();
            MonitorChoice {
                index,
                name: monitor.name(),
                size_px: (size.width, size.height),
            }
        })
        .collect()
}

fn lock_console(state: &Arc<Mutex<ConsoleState>>) -> MutexGuard<'_, ConsoleState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn log_warning(shared: &Arc<Mutex<ConsoleState>>, message: impl Into<String>) {
    lock_console(shared).log(LogLevel::Warning, message);
}

fn log_error(shared: &Arc<Mutex<ConsoleState>>, message: impl Into<String>) {
    lock_console(shared).log(LogLevel::Error, message);
}

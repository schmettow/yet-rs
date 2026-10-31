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
//! `F3` toggles the enlarged adjust view, `Esc` cancels calibration, `F8` aborts
//! the session, and `F4` resets a finished or aborted session to idle. The
//! camera and Quadbright picture-in-picture toggles are console checkboxes and
//! presenter-only `P`/`Q`.
//!
//! On startup the console is centered on the monitor where the window manager
//! placed it, and the presenter is moved to another monitor when the configured
//! presenter monitor would share the console output.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eframe::egui;
use yet_core::{GazePoint, QuadBrightness};
use yet_egui::console::{
    CameraChoice, ConsoleState, ConsoleUi, ExperimentCommand, LogLevel, MonitorChoice, PreviewFrame,
};
use yet_egui::{AdjustFrame, EguiOverlay, EguiPresenter, PresenterConfig, PresenterPreview};
use yet_stim::layout::StimulusPlacement;
use yet_stim::{
    gaze_scene, step, CalibrationGrid, CalibrationPhase, CalibrationRun, CameraConfig,
    ConsoleEvent, Effect, EngineEvent, EngineState, FrameRecord, GazeMarkerStyle, GazeSample,
    Phase, QuickBackground, SessionPlan,
};
use yet_stim::{Color, OverlayBackend, Scene, ScreenGeometry, SessionTime};

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

/// How many frames to repeat the windowed presenter position command.
///
/// The first command can arrive before the deferred viewport is mapped and is
/// then dropped; repeating it also corrects cross-DPI rounding.
const POSITION_ATTEMPTS: u8 = 20;

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
    /// Start in the enlarged camera adjust view.
    pub adjust: bool,
}

impl<'a> ConsoleOptions<'a> {
    /// Create options for an experiment directory.
    pub fn new(experiment_dir: &'a Path) -> Self {
        Self {
            experiment_dir,
            participant: None,
            output: None,
            device: None,
            adjust: false,
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

    /// Start in the enlarged camera adjust view.
    pub fn adjust(mut self, adjust: bool) -> Self {
        self.adjust = adjust;
        self
    }
}

/// Open the console and run until the experimenter closes it.
pub fn run_console(options: ConsoleOptions<'_>) -> Result<(), RunError> {
    let prepared = prepare(options.experiment_dir, options.participant, options.output)?;
    let experiment_id = prepared.loaded.experiment.id.clone();
    let display = prepared.loaded.config.display.clone();
    let mut camera_config = prepared.loaded.config.camera.clone();
    let initial_device = options.device.unwrap_or(camera_config.usb_index);
    camera_config.usb_index = initial_device;
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

    let mut audio = yet_stim::media::open_player(
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
        console.preview.adjust = options.adjust;
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
            "keys: F5 start · F6 pause/continue · F7 calibrate · F9 quick · F10 preview · F3 adjust · F4 reset · F8 exit safeguard · F follow",
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
    refresh_camera_devices(&shared);

    let (commands, controller) = spawn_controller(
        prepared.loaded.plan,
        prepared.sink,
        presenter.clone(),
        Arc::clone(&shared),
        camera_config,
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
        position_attempts: 0,
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
    position_attempts: u8,
}

impl eframe::App for ConsoleApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // The overlay viewport uses this clear color; the console paints its own
        // opaque background below.
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let rescan = {
            let mut console = lock_console(&self.shared);
            let rescan = console.camera_rescan;
            console.camera_rescan = false;
            rescan
        };
        if rescan {
            refresh_camera_devices(&self.shared);
        }
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, egui::Color32::from_rgb(12, 12, 12));
        if !self.sized {
            self.place_windows(&ctx, frame);
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
            let presenter = yet_egui::console::PresenterStatus {
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
            self.position_attempts = 0;
            let position = if fullscreen {
                None
            } else {
                self.position_presenter(frame, monitor)
            };
            self.presenter.set_position_pt(position);
            let mut console = lock_console(&self.shared);
            console.log(
                LogLevel::Info,
                format!("presenter monitor {monitor}, fullscreen {fullscreen}"),
            );
        }

        yet_egui::show_viewport(&ctx, &self.presenter.shared());
        if fullscreen {
            self.position_attempts = POSITION_ATTEMPTS;
        } else if self.position_attempts < POSITION_ATTEMPTS {
            if let Some((x, y)) = self.position_presenter(frame, monitor) {
                ctx.send_viewport_cmd_to(
                    yet_egui::viewport_id(monitor),
                    egui::ViewportCommand::OuterPosition(egui::pos2(x, y)),
                );
                self.position_attempts += 1;
            }
        }
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
    /// Center the console on the monitor where it was launched and keep the
    /// presenter on a different monitor when both would share one.
    fn place_windows(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let Some(window) = frame.winit_window() else {
            return;
        };
        let Some(current) = window.current_monitor() else {
            return;
        };
        let monitors: Vec<_> = window.available_monitors().collect();
        let Some(console_index) = monitors.iter().position(|monitor| monitor == &current) else {
            return;
        };
        let pixels_per_point = ctx.pixels_per_point().max(0.1);

        // Center the existing window on its monitor. Only the position is
        // changed: re-requesting the inner size here would use a first-frame
        // scale factor that can differ from the platform's stable value.
        let monitor_pos = current.position();
        let monitor_size = current.size();
        let outer = window.outer_size();
        let monitor_pt =
            egui::vec2(monitor_size.width as f32, monitor_size.height as f32) / pixels_per_point;
        let outer_pt = egui::vec2(outer.width as f32, outer.height as f32) / pixels_per_point;
        let margin = 24.0;
        let origin = centered_origin(
            (
                monitor_pos.x as f32 / pixels_per_point,
                monitor_pos.y as f32 / pixels_per_point,
            ),
            (monitor_pt.x, monitor_pt.y),
            (outer_pt.x, outer_pt.y),
            margin,
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            origin.0, origin.1,
        )));

        let (presenter_monitor, fullscreen) = {
            let mut console = lock_console(&self.shared);
            let configured = console.presenter_monitor;
            let presenter_monitor =
                presenter_monitor_for(configured, console_index, monitors.len());
            if presenter_monitor != configured {
                console.presenter_monitor = presenter_monitor;
            }
            console.log(
                LogLevel::Info,
                format!(
                    "console {:.0}×{:.0} pt centered on monitor {console_index} ({pixels_per_point:.2} px/pt); presenter on monitor {presenter_monitor}",
                    outer_pt.x, outer_pt.y
                ),
            );
            (presenter_monitor, console.fullscreen)
        };

        if fullscreen {
            self.presenter.set_position_pt(None);
        } else if let Some(position) = self.position_presenter(frame, presenter_monitor) {
            self.presenter.set_position_pt(Some(position));
        }
        self.sized = true;
    }

    /// Center the windowed presenter on `monitor_index`.
    fn position_presenter(
        &self,
        frame: &eframe::Frame,
        monitor_index: usize,
    ) -> Option<(f32, f32)> {
        let window = frame.winit_window()?;
        let monitor = window.available_monitors().nth(monitor_index)?;
        let scale = monitor.scale_factor() as f32;
        let position = monitor.position();
        let size = monitor.size();
        let monitor_pt = (size.width as f32 / scale, size.height as f32 / scale);
        let window_pt = self.presenter.window_size_pt();
        Some(centered_origin(
            (position.x as f32 / scale, position.y as f32 / scale),
            monitor_pt,
            window_pt,
            24.0,
        ))
    }

    fn update_overlay(&mut self, ctx: &egui::Context, status: &yet_egui::PresenterStatus) {
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
pub(crate) fn spawn_controller(
    plan: SessionPlan,
    mut sink: yet_stim::DataSink<std::io::BufWriter<std::fs::File>>,
    mut presenter: EguiPresenter,
    shared: Arc<Mutex<ConsoleState>>,
    mut camera_config: CameraConfig,
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
                &mut camera_config,
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
    camera_config: &mut CameraConfig,
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
    let mut adjust_generation = 0_u64;
    let mut last_preview: Option<Instant> = None;
    let mut first_trial = false;
    let mut auto_quick = false;
    let overlay_style = GazeMarkerStyle::default();
    let mut overlay_visible = false;
    let mut overlay_gaze: Option<GazePoint> = None;
    let mut overlay_warned = false;
    let mut follow_was_active = false;
    let mut last_follow = false;

    while !shut_down {
        let selected_camera = lock_console(shared).camera_usb_index;
        if selected_camera != camera_config.usb_index {
            let active = matches!(state.phase, Phase::Running | Phase::Paused)
                || calibration.as_ref().is_some_and(CalibrationRun::is_active);
            if active {
                let mut console = lock_console(shared);
                console.camera_usb_index = camera_config.usb_index;
                console.camera_error =
                    Some("stop or pause the session before changing camera".to_owned());
                console.log(
                    LogLevel::Warning,
                    "camera selection is locked while the session is active",
                );
            } else {
                camera_config.usb_index = selected_camera;
                tracker = None;
                calibration = None;
                latest_gaze.set(None);
                let mut console = lock_console(shared);
                console.preview.frame = None;
                console.preview.error = None;
                console.preview.capture_fps = None;
                console.adjusted = false;
                console.calibration = Default::default();
                console.overlay.gaze = None;
                console.camera_error = None;
                console.log(
                    LogLevel::Info,
                    format!("camera switched to index {selected_camera}; recalibrate"),
                );
            }
        }

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

        // Console intent: preview on/off, gaze circle and a retry request.
        let (preview_enabled, pip_stream, adjust, retry, follow) = {
            let console = lock_console(shared);
            (
                console.preview.enabled,
                console.preview.pip_stream,
                console.preview.adjust,
                console.preview.retry,
                console.follow,
            )
        };
        let preview_active = preview_enabled || pip_stream || adjust;
        if retry {
            let mut console = lock_console(shared);
            console.preview.retry = false;
            console.preview.error = None;
            drop(console);
            tracker = None;
        }
        if follow != last_follow {
            last_follow = follow;
            lock_console(shared).log(
                LogLevel::Info,
                format!("follow {}", if follow { "on" } else { "off" }),
            );
        }

        let calibrating = calibration.as_ref().is_some_and(CalibrationRun::is_active);
        let running = state.phase == Phase::Running;
        let fitted = calibration.as_ref().is_some_and(CalibrationRun::is_fitted);
        if (preview_active || (follow && fitted)) && tracker.is_none() {
            tracker = open_tracker(camera_config, shared);
        }

        let want_features = calibrating || preview_active || (fitted && (running || follow));
        let mut features: Option<QuadBrightness> = None;

        if want_features {
            if let Some(tracker) = tracker.as_mut() {
                if let Some(polled) = tracker.poll() {
                    features = Some(polled.features);
                    if fitted && (running || calibrating || follow) {
                        if let Some(run) = &calibration {
                            if let Ok(point) = run.predict(polled.features) {
                                latest_gaze.set(Some(point));
                                if running {
                                    lock_console(shared).overlay.gaze = Some(point);
                                }
                            }
                        }
                    }
                    if preview_active
                        && last_preview
                            .is_none_or(|last| now.duration_since(last) >= PREVIEW_INTERVAL)
                    {
                        update_preview(
                            shared,
                            tracker,
                            &polled,
                            calibration.as_ref(),
                            runner.geometry().size_px,
                            &mut preview_generation,
                        );
                        if adjust {
                            update_adjust_frame(shared, tracker, &polled, &mut adjust_generation);
                        }
                        last_preview = Some(now);
                    }
                    if preview_active {
                        let mut console = lock_console(shared);
                        console.preview.error = None;
                        console.preview.capture_fps = tracker.capture_fps();
                    }
                } else if preview_active {
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
        sync_presenter_preview(runner, shared, pip_stream);
        sync_presenter_adjust(runner, shared, adjust);

        // Wait for a command or the next tick.
        let active = running || calibrating || (follow && fitted);
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
            Ok(command) if adjust_blocks(shared, command) => {
                log_warning(shared, "finish adjusting the camera first");
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
                                tracker = open_tracker(camera_config, shared);
                            }
                            if !calibration.as_ref().is_some_and(CalibrationRun::is_fitted) {
                                log_warning(
                                    shared,
                                    "starting without calibration; no gaze rows will be written",
                                );
                            }
                            let mut effects = step(state, EngineEvent::Start, at);
                            let trial_effect = split_trial_start(&mut effects);
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
                            } else if let Some(effect) = trial_effect {
                                if let Err(error) = apply_effects(
                                    state,
                                    runner,
                                    vec![effect],
                                    shared,
                                    at,
                                    start,
                                    &mut first_trial,
                                ) {
                                    fail(state, runner, shared, error);
                                } else {
                                    match begin_auto_quick(
                                        start,
                                        at,
                                        state,
                                        runner,
                                        &mut calibration,
                                        &mut tracker,
                                        camera_config,
                                        shared,
                                        &mut last_calibration_phase,
                                        &mut last_calibration_target,
                                    ) {
                                        Ok(started) => auto_quick = started,
                                        Err(error) => fail(state, runner, shared, error),
                                    }
                                }
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
                            auto_quick = false;
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
                    ExperimentCommand::Reset => {
                        if calibrating {
                            cancel_calibration(state, runner, calibration.as_mut(), shared, start);
                        }
                        let effects = step(state, EngineEvent::Reset, at);
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
                        runner.reset_report();
                        update_progress(shared, runner.report(), state, at);
                        first_trial = false;
                        auto_quick = false;
                        {
                            let mut console = lock_console(shared);
                            console.error = None;
                            console.elapsed = Duration::ZERO;
                            console.current_block = None;
                            console.current_trial = None;
                            console.current_stimulus = None;
                            console
                                .log(LogLevel::Info, "session reset; ready to calibrate or start");
                        }
                        last_tick = Instant::now();
                        next_tick = last_tick;
                    }
                    ExperimentCommand::Calibrate => {
                        if calibrating {
                            log_warning(shared, "a calibration is already running");
                        } else if !matches!(
                            state.phase,
                            Phase::Idle | Phase::Paused | Phase::Finished
                        ) {
                            log_warning(
                                shared,
                                "a full calibration is available when the session is idle or paused",
                            );
                        } else {
                            clear_gaze(shared, latest_gaze);
                            start_full_calibration(
                                start,
                                runner,
                                &mut calibration,
                                &mut tracker,
                                camera_config,
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
                            clear_gaze(shared, latest_gaze);
                            start_quick_calibration(
                                start,
                                state,
                                runner,
                                &mut calibration,
                                &mut tracker,
                                camera_config,
                                shared,
                                &mut last_calibration_phase,
                                &mut last_calibration_target,
                                false,
                            );
                            last_tick = Instant::now();
                            next_tick = last_tick;
                        }
                    }
                    ExperimentCommand::CancelCalibration => {
                        let cancelled =
                            cancel_calibration(state, runner, calibration.as_mut(), shared, start);
                        if cancelled && auto_quick {
                            auto_quick = false;
                            if let Err(error) = resume_after_auto_quick(state, runner, shared, at) {
                                fail(state, runner, shared, error);
                            }
                        }
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
            let marker = follow.then(|| latest_gaze.get()).flatten();
            runner.present_scene_with_marker(at, dt, &scene, marker);
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
            if finished && auto_quick {
                auto_quick = false;
                if let Err(error) = resume_after_auto_quick(state, runner, shared, at) {
                    fail(state, runner, shared, error);
                    continue;
                }
            } else if failed {
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

        // Outside a running trial, a fitted follow view is a live tracker
        // check: keep the camera polling and repaint a neutral (or current)
        // scene with the circle. This is what makes `F` useful before Space.
        let idle_follow = follow && fitted && state.phase != Phase::Running;
        if state.phase != Phase::Running && (idle_follow || follow_was_active) {
            follow_was_active = idle_follow;
            if now < next_tick {
                continue;
            }
            let at = session_time(start, now);
            let dt = now
                .duration_since(last_tick)
                .min(TICK)
                .max(Duration::from_micros(1));
            last_tick = now;
            let scene = if runner.scene().commands().is_empty() {
                let mut scene = Scene::new();
                scene.clear(Color::WHITE);
                scene
            } else {
                runner.scene().clone()
            };
            let marker = if idle_follow { latest_gaze.get() } else { None };
            runner.present_scene_with_marker(at, dt, &scene, marker);
            next_tick += TICK;
            if next_tick <= now {
                next_tick = now + TICK;
            }
            continue;
        }
        follow_was_active = false;

        if state.phase != Phase::Running || now < next_tick {
            continue;
        }

        let at = session_time(start, now);
        let dt = now.duration_since(last_tick).max(Duration::from_micros(1));
        last_tick = now;
        let mut effects = step(state, EngineEvent::Tick, at);
        let trial_effect = split_trial_start(&mut effects);
        if let Err(error) =
            apply_effects(state, runner, effects, shared, at, start, &mut first_trial)
        {
            fail(state, runner, shared, error);
            continue;
        }
        if let Some(effect) = trial_effect {
            if let Err(error) = apply_effects(
                state,
                runner,
                vec![effect],
                shared,
                at,
                start,
                &mut first_trial,
            ) {
                fail(state, runner, shared, error);
                continue;
            }
            match begin_auto_quick(
                start,
                at,
                state,
                runner,
                &mut calibration,
                &mut tracker,
                camera_config,
                shared,
                &mut last_calibration_phase,
                &mut last_calibration_target,
            ) {
                Ok(started) => auto_quick = started,
                Err(error) => {
                    fail(state, runner, shared, error);
                    continue;
                }
            }
        }
        if state.phase == Phase::Running {
            let experiment_t = SessionTime::from_duration(state.elapsed(at).unwrap_or_default());
            let marker = follow.then(|| latest_gaze.get()).flatten();
            let stats = runner.present_with_marker(experiment_t, dt, marker);
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

/// Whether an adjust view blocks a session or calibration command.
fn adjust_blocks(shared: &Arc<Mutex<ConsoleState>>, command: ExperimentCommand) -> bool {
    matches!(
        command,
        ExperimentCommand::Start
            | ExperimentCommand::Resume
            | ExperimentCommand::Calibrate
            | ExperimentCommand::QuickCalibrate
    ) && lock_console(shared).preview.adjust
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
    shared: &Arc<Mutex<ConsoleState>>,
    last_phase: &mut CalibrationPhase,
    last_target: &mut Option<usize>,
) where
    G: FnMut(&FrameRecord, Option<StimulusPlacement>) -> Option<GazeSample>,
{
    if tracker.is_none() {
        *tracker = open_tracker(camera_config, shared);
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
    shared: &Arc<Mutex<ConsoleState>>,
    last_phase: &mut CalibrationPhase,
    last_target: &mut Option<usize>,
    automatic: bool,
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
        *tracker = open_tracker(camera_config, shared);
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
                    if automatic {
                        "automatic quick recalibration: look at the orange center target"
                    } else {
                        "quick recalibration: look at the orange center target"
                    },
                );
                if !automatic && state.phase == Phase::Paused {
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

/// Remove a `TrialStarted` effect from a batch so the controller can gate the
/// trial behind an automatic quick recalibration.
fn split_trial_start(effects: &mut Vec<Effect>) -> Option<Effect> {
    let index = effects
        .iter()
        .position(|effect| matches!(effect, Effect::TrialStarted { .. }))?;
    Some(effects.remove(index))
}

/// Pause a freshly started trial and run a quick recalibration over its blurred
/// stimulus, mirroring the Python `prepareStimulus`/`Quick` states.
///
/// The engine keeps the trial deadline fixed while paused; resuming shifts it by
/// the duration of the recalibration. Returns whether a run is now active.
#[allow(clippy::too_many_arguments)]
fn begin_auto_quick<G>(
    start: Instant,
    at: SessionTime,
    state: &mut EngineState,
    runner: &mut SessionRunner<'_, std::io::BufWriter<std::fs::File>, EguiPresenter, G>,
    calibration: &mut Option<CalibrationRun>,
    tracker: &mut Option<Tracker>,
    camera_config: &CameraConfig,
    shared: &Arc<Mutex<ConsoleState>>,
    last_phase: &mut CalibrationPhase,
    last_target: &mut Option<usize>,
) -> Result<bool, RunError>
where
    G: FnMut(&FrameRecord, Option<StimulusPlacement>) -> Option<GazeSample>,
{
    let eligible = calibration
        .as_ref()
        .is_some_and(|run| run.is_fitted() && !run.is_active());
    if !eligible {
        return Ok(false);
    }
    if tracker.is_none() {
        *tracker = open_tracker(camera_config, shared);
    }
    if tracker.is_none() {
        return Ok(false);
    }
    let surface = runner.geometry().size_px;
    if calibration
        .as_ref()
        .is_some_and(|run| run.grid().surface_px() != surface)
    {
        log_warning(
            shared,
            "the presenter surface changed; run a full calibration first",
        );
        return Ok(false);
    }

    for effect in step(state, EngineEvent::Pause, at) {
        record_effect(shared, &effect);
        runner.apply(effect)?;
    }
    start_quick_calibration(
        start,
        state,
        runner,
        calibration,
        tracker,
        camera_config,
        shared,
        last_phase,
        last_target,
        true,
    );
    if calibration.as_ref().is_some_and(CalibrationRun::is_active) {
        Ok(true)
    } else {
        resume_after_auto_quick(state, runner, shared, at)?;
        Ok(false)
    }
}

/// Resume a trial that was paused for an automatic quick recalibration.
fn resume_after_auto_quick<G>(
    state: &mut EngineState,
    runner: &mut SessionRunner<'_, std::io::BufWriter<std::fs::File>, EguiPresenter, G>,
    shared: &Arc<Mutex<ConsoleState>>,
    at: SessionTime,
) -> Result<(), RunError>
where
    G: FnMut(&FrameRecord, Option<StimulusPlacement>) -> Option<GazeSample>,
{
    if state.phase != Phase::Paused {
        return Ok(());
    }
    for effect in step(state, EngineEvent::Resume, at) {
        record_effect(shared, &effect);
        runner.apply(effect)?;
    }
    lock_console(shared).log(
        LogLevel::Info,
        "quick recalibration done; the trial resumes",
    );
    Ok(())
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
    polled: &crate::tracker::PolledFrame,
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
        quad: Some(polled.features),
        gaze,
        gaze_normalized,
    };
    let mut console = lock_console(shared);
    console.preview.frame = Some(frame);
    console.preview.error = None;
    console.preview.capture_fps = tracker.capture_fps();
}

/// Mirror the camera stream PiP flag and frame onto the presenter.
fn sync_presenter_preview<G>(
    runner: &mut SessionRunner<'_, std::io::BufWriter<std::fs::File>, EguiPresenter, G>,
    shared: &Arc<Mutex<ConsoleState>>,
    pip_stream: bool,
) where
    G: FnMut(&FrameRecord, Option<StimulusPlacement>) -> Option<GazeSample>,
{
    let preview = if pip_stream {
        let console = lock_console(shared);
        console
            .preview
            .frame
            .as_ref()
            .map(|frame| PresenterPreview {
                width: frame.width,
                height: frame.height,
                data: Arc::clone(&frame.data),
                generation: frame.generation,
                show_stream: true,
            })
    } else {
        None
    };
    runner.presenter_mut().set_preview(preview);
}

/// Update the full camera frame shown while adjusting.
fn update_adjust_frame(
    shared: &Arc<Mutex<ConsoleState>>,
    tracker: &Tracker,
    polled: &crate::tracker::PolledFrame,
    generation: &mut u64,
) {
    let Some((width, height, data)) = crate::tracker::thumbnail(&polled.frame, 960, 720) else {
        return;
    };
    let frame_width = polled.frame.width();
    let frame_height = polled.frame.height();
    if frame_width == 0 || frame_height == 0 {
        return;
    }
    let roi = tracker.roi();
    *generation += 1;
    lock_console(shared).preview.adjust_frame = Some(AdjustFrame {
        width,
        height,
        data: Arc::from(data),
        generation: *generation,
        roi: [
            roi.x as f32 / frame_width as f32,
            roi.y as f32 / frame_height as f32,
            roi.width as f32 / frame_width as f32,
            roi.height as f32 / frame_height as f32,
        ],
    });
}

/// Show or hide the enlarged adjust view on the presenter.
fn sync_presenter_adjust<G>(
    runner: &mut SessionRunner<'_, std::io::BufWriter<std::fs::File>, EguiPresenter, G>,
    shared: &Arc<Mutex<ConsoleState>>,
    adjust: bool,
) where
    G: FnMut(&FrameRecord, Option<StimulusPlacement>) -> Option<GazeSample>,
{
    let frame = if adjust {
        lock_console(shared).preview.adjust_frame.clone()
    } else {
        None
    };
    runner.presenter_mut().set_adjust(frame);
}

fn refresh_camera_devices(shared: &Arc<Mutex<ConsoleState>>) {
    let result = crate::camera::list_devices();
    let mut console = lock_console(shared);
    match result {
        Ok(devices) => {
            console.camera_devices = devices
                .into_iter()
                .map(|device| CameraChoice {
                    index: device.index as usize,
                    name: device.name,
                    detail: device.detail,
                })
                .collect();
            console.camera_error = None;
            if console.camera_devices.is_empty() {
                console.log(LogLevel::Warning, "camera scan found no devices");
            }
        }
        Err(error) => {
            let message = error.to_string();
            console.camera_error = Some(message.clone());
            console.log(LogLevel::Warning, format!("camera scan: {message}"));
        }
    }
}

fn open_tracker(config: &CameraConfig, shared: &Arc<Mutex<ConsoleState>>) -> Option<Tracker> {
    match Tracker::open(config, None) {
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

pub(crate) fn plan_has_audio(plan: &SessionPlan) -> bool {
    plan.blocks
        .iter()
        .flat_map(|block| &block.trials)
        .any(|planned| planned.trial.stimulus.audio.is_some())
}

/// Pick the presenter monitor.
///
/// A configured monitor that differs from the console is respected; when both
/// would share an output, the next output is used instead.
fn presenter_monitor_for(configured: usize, console_index: usize, available: usize) -> usize {
    if available > 1 && configured == console_index {
        (console_index + 1) % available
    } else {
        configured
    }
}

/// Center a window inside a monitor and keep its top edge reachable.
fn centered_origin(
    monitor_pos: (f32, f32),
    monitor_size: (f32, f32),
    window_size: (f32, f32),
    top_margin: f32,
) -> (f32, f32) {
    let x = monitor_pos.0 + (monitor_size.0 - window_size.0).max(0.0) * 0.5;
    let y = monitor_pos.1 + (monitor_size.1 - window_size.1).max(0.0) * 0.5;
    (x, y.max(monitor_pos.1 + top_margin))
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

pub(crate) fn lock_console(state: &Arc<Mutex<ConsoleState>>) -> MutexGuard<'_, ConsoleState> {
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use yet_stim::{BlockId, PresenterCommand, Scene, Stimulus, StimulusId, StimulusKind};

    use super::{centered_origin, presenter_monitor_for, split_trial_start, Effect};

    fn trial_start() -> Effect {
        Effect::TrialStarted {
            block: BlockId::from("B1"),
            trial_index: 0,
            stimulus: Stimulus {
                id: StimulusId::from("face"),
                kind: StimulusKind::Image,
                path: PathBuf::from("stimuli/face.png"),
                duration: Duration::from_secs(3),
                fps: None,
                looped: false,
                audio: None,
            },
        }
    }

    #[test]
    fn the_trial_start_is_split_out_and_other_effects_keep_their_order() {
        let mut effects = vec![
            Effect::Present(Scene::new()),
            trial_start(),
            Effect::Presenter(PresenterCommand::Stop),
        ];

        let trial = split_trial_start(&mut effects).expect("a trial start");

        assert!(matches!(trial, Effect::TrialStarted { .. }));
        assert_eq!(effects.len(), 2);
        assert!(matches!(effects[0], Effect::Present(_)));
        assert!(matches!(effects[1], Effect::Presenter(_)));
    }

    #[test]
    fn the_presenter_moves_off_the_console_monitor() {
        assert_eq!(presenter_monitor_for(0, 0, 2), 1);
        assert_eq!(presenter_monitor_for(1, 1, 2), 0);
        assert_eq!(presenter_monitor_for(0, 1, 2), 0);
        assert_eq!(presenter_monitor_for(0, 0, 1), 0);
    }

    #[test]
    fn a_centered_window_keeps_its_title_bar_reachable() {
        assert_eq!(
            centered_origin((2560.0, 0.0), (1440.0, 2560.0), (900.0, 900.0), 24.0),
            (2830.0, 830.0)
        );
        assert_eq!(
            centered_origin((0.0, 0.0), (1400.0, 800.0), (1400.0, 860.0), 24.0),
            (0.0, 24.0)
        );
        assert_eq!(
            centered_origin((0.0, 0.0), (1920.0, 1080.0), (1400.0, 860.0), 24.0),
            (260.0, 110.0)
        );
    }

    #[test]
    fn a_tick_without_a_new_trial_yields_no_gate() {
        let mut effects = vec![Effect::Presenter(PresenterCommand::Pause)];

        assert!(split_trial_start(&mut effects).is_none());
        assert_eq!(effects.len(), 1);
    }
}

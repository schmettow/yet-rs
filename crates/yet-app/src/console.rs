// SPDX-License-Identifier: GPL-3.0-only

//! Graphical experimenter console plus the stimulus window.
//!
//! One eframe event loop owns two viewports: the root viewport is the console,
//! and a deferred viewport on the configured monitor presents stimuli. A
//! dedicated controller thread owns the engine, the data sink, and the
//! presenter handle, so console repaints never drive experiment time.
//!
//! Keyboard shortcuts: `F5` starts, `F6` pauses or resumes, `F8` aborts.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eframe::egui;
use yet_console::{ConsoleState, ConsoleUi, ExperimentCommand, LogLevel};
use yet_core::{
    step, ConsoleEvent, Effect, EngineEvent, EngineState, FrameRecord, GazeSample, Phase,
    SessionPlan,
};
use yet_present::SessionTime;
use yet_present_egui::{EguiPresenter, PresenterConfig};

use crate::driver::{prepare, RunError, RunReport, SessionRunner};

/// Frame step of the real-time controller.
const TICK: Duration = Duration::from_micros(16_667);

/// How often the console repaints while a session is active.
const CONSOLE_REPAINT: Duration = Duration::from_millis(100);

/// Options for [`run_console`].
#[derive(Clone, Copy, Debug)]
pub struct ConsoleOptions<'a> {
    /// Experiment directory containing `config.toml` and the CSVs.
    pub experiment_dir: &'a Path,
    /// Participant identifier; defaults to the Unix time at session start.
    pub participant: Option<&'a str>,
    /// Result CSV path; defaults to the experiment directory.
    pub output: Option<&'a Path>,
}

impl<'a> ConsoleOptions<'a> {
    /// Create options for an experiment directory.
    pub fn new(experiment_dir: &'a Path) -> Self {
        Self {
            experiment_dir,
            participant: None,
            output: None,
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
}

/// Open the console and run until the experimenter closes it.
pub fn run_console(options: ConsoleOptions<'_>) -> Result<(), RunError> {
    let prepared = prepare(options.experiment_dir, options.participant, options.output)?;
    let experiment_id = prepared.loaded.experiment.id.clone();
    let display = prepared.loaded.config.display.clone();
    let trials = prepared.loaded.plan.trial_count();
    let participant = prepared.sink.identity().participant.clone();

    let presenter = EguiPresenter::new(PresenterConfig::new(
        display.presenter_monitor,
        display.fullscreen,
        (display.width, display.height),
        1.0,
    ));
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
        console.log(LogLevel::Info, "waiting for the stimulus window…");
    }

    let (commands, controller) = spawn_controller(
        prepared.loaded.plan,
        prepared.sink,
        presenter.clone(),
        Arc::clone(&shared),
    )?;

    let app = ConsoleApp {
        shared,
        presenter,
        commands,
        controller: Some(controller),
        console: ConsoleUi::new(),
        logged_ready: false,
    };

    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("yet-rs console · {experiment_id}"))
            .with_app_id("yet-rs-console")
            .with_inner_size([560.0, 780.0])
            .with_min_inner_size([420.0, 480.0]),
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
    logged_ready: bool,
}

impl eframe::App for ConsoleApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let status = self.presenter.status();

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
            self.console.ui(ui, &mut console, presenter)
        };
        for command in requested {
            let focus_presenter = matches!(
                command,
                ExperimentCommand::Start | ExperimentCommand::Resume
            );
            let _ = self.commands.send(command);
            if focus_presenter {
                // KWin's "Dim Inactive" effect darkens an unfocused window by
                // 25%, which would change the stimulus luminance. Ask the
                // stimulus viewport itself to take focus for the trial.
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
        ctx.request_repaint_after(CONSOLE_REPAINT);
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

fn spawn_controller(
    plan: SessionPlan,
    mut sink: yet_core::DataSink<std::io::BufWriter<std::fs::File>>,
    mut presenter: EguiPresenter,
    shared: Arc<Mutex<ConsoleState>>,
) -> Result<(Sender<ExperimentCommand>, JoinHandle<()>), RunError> {
    let (commands, receiver) = mpsc::channel();
    let handle = thread::Builder::new()
        .name("yet-session".to_owned())
        .spawn(move || {
            let trials = plan.trial_count();
            let mut state = EngineState::new(plan);
            let mut runner = SessionRunner::new(&mut sink, &mut presenter, |_: &FrameRecord| None);
            runner.report_mut().trials = trials;
            let start = Instant::now();
            run_loop(start, &mut state, &mut runner, &receiver, &shared);

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

fn run_loop<G>(
    start: Instant,
    state: &mut EngineState,
    runner: &mut SessionRunner<'_, std::io::BufWriter<std::fs::File>, EguiPresenter, G>,
    commands: &Receiver<ExperimentCommand>,
    shared: &Arc<Mutex<ConsoleState>>,
) where
    G: FnMut(&FrameRecord) -> Option<GazeSample>,
{
    let mut next_tick = Instant::now();
    let mut last_tick = next_tick;
    let mut shut_down = false;

    while !shut_down {
        let now = Instant::now();
        let active = state.phase == Phase::Running;
        let wait = if active {
            next_tick.saturating_duration_since(now)
        } else {
            Duration::from_millis(50)
        }
        .max(Duration::from_millis(1));

        match commands.recv_timeout(wait) {
            Ok(ExperimentCommand::Shutdown) => {
                abort_if_active(start, state, runner, shared);
                shut_down = true;
            }
            Ok(command) => {
                let event = match command {
                    ExperimentCommand::Start => EngineEvent::Start,
                    ExperimentCommand::Pause => EngineEvent::Pause,
                    ExperimentCommand::Resume => EngineEvent::Resume,
                    ExperimentCommand::Abort => EngineEvent::Abort,
                    ExperimentCommand::Shutdown => unreachable!(),
                };
                let at = session_time(start, Instant::now());
                let effects = step(state, event, at);
                if let Err(error) = apply_effects(state, runner, effects, shared, at) {
                    fail(state, runner, shared, error);
                }
                last_tick = Instant::now();
                next_tick = last_tick;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                abort_if_active(start, state, runner, shared);
                shut_down = true;
            }
        }

        if shut_down {
            break;
        }

        let now = Instant::now();
        if state.phase != Phase::Running || now < next_tick {
            continue;
        }

        let at = session_time(start, now);
        let dt = now.duration_since(last_tick).max(Duration::from_micros(1));
        last_tick = now;
        let effects = step(state, EngineEvent::Tick, at);
        if let Err(error) = apply_effects(state, runner, effects, shared, at) {
            fail(state, runner, shared, error);
            continue;
        }
        if state.phase == Phase::Running {
            let experiment_t = SessionTime::from_duration(state.elapsed(at).unwrap_or_default());
            runner.present(experiment_t, dt);
        }

        next_tick += TICK;
        if next_tick <= now {
            next_tick = now + TICK;
        }
    }
}

fn abort_if_active<G>(
    start: Instant,
    state: &mut EngineState,
    runner: &mut SessionRunner<'_, std::io::BufWriter<std::fs::File>, EguiPresenter, G>,
    shared: &Arc<Mutex<ConsoleState>>,
) where
    G: FnMut(&FrameRecord) -> Option<GazeSample>,
{
    if matches!(state.phase, Phase::Running | Phase::Paused) {
        let at = session_time(start, Instant::now());
        let effects = step(state, EngineEvent::Abort, at);
        if let Err(error) = apply_effects(state, runner, effects, shared, at) {
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
    G: FnMut(&FrameRecord) -> Option<GazeSample>,
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
) -> Result<(), RunError>
where
    G: FnMut(&FrameRecord) -> Option<GazeSample>,
{
    update_progress(shared, runner.report(), state, at);
    for effect in effects {
        record_effect(shared, &effect);
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

fn lock_console(state: &Arc<Mutex<ConsoleState>>) -> MutexGuard<'_, ConsoleState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// SPDX-License-Identifier: GPL-3.0-only

//! The pure experiment engine.
//!
//! [`step`] is the only state transition. It never sleeps, performs I/O, or
//! touches a UI. The application owns the loop and executes the returned
//! [`Effect`]s. This is the replacement for Yex's blocking `run()` methods.

use std::time::Duration;

use thiserror::Error;
use yet_present::{MediaId, PresenterCommand, Scene, SessionTime};

use crate::tracking::{CalibrationSample, GazePoint};

/// Lifecycle phase of an experiment session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// No session has started.
    #[default]
    Idle,
    /// Trials are running.
    Running,
    /// The session is paused; experiment time does not advance.
    Paused,
    /// The session completed normally.
    Finished,
    /// The session was aborted.
    Aborted,
}

/// Engine state. This is data, not a runtime handle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EngineState {
    /// Current lifecycle phase.
    pub phase: Phase,
    /// Index of the active trial.
    pub trial_index: usize,
    /// Session time at which the session started.
    pub session_start: Option<SessionTime>,
    /// Session time of the most recent tick.
    pub last_tick: Option<SessionTime>,
    /// Session time at which the current pause began.
    pub paused_at: Option<SessionTime>,
    /// Total time spent paused during this session.
    pub paused_total: Duration,
}

impl EngineState {
    /// Experiment-relative time at `now`, excluding completed and current pauses.
    ///
    /// Returns `None` before the session starts.
    pub fn elapsed(&self, now: SessionTime) -> Option<Duration> {
        let start = self.session_start?;
        let mut elapsed = now.duration_since(start).saturating_sub(self.paused_total);
        if let Some(paused_at) = self.paused_at {
            elapsed = elapsed.saturating_sub(now.duration_since(paused_at));
        }
        Some(elapsed)
    }
}

/// A keyboard key relevant to the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// Space bar.
    Space,
    /// Backspace.
    Backspace,
    /// Escape; aborts an active session.
    Escape,
    /// Any other character.
    Char(char),
}

/// An error produced by the engine.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum EngineError {
    /// The event is not valid in the current phase.
    #[error("event `{event}` is not valid while the engine is {phase:?}")]
    InvalidTransition {
        /// Phase in which the event arrived.
        phase: Phase,
        /// Name of the rejected event.
        event: &'static str,
    },
    /// An external component reported an error before the session started.
    #[error("the engine has not started")]
    NotStarted,
}

/// An input or lifecycle event delivered to the engine.
#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    /// Start a session.
    Start,
    /// Pause an active session.
    Pause,
    /// Resume a paused session.
    Resume,
    /// Abort the session.
    Abort,
    /// Advance the clock.
    Tick,
    /// A keyboard event from the presenter or console.
    Key(Key),
    /// A calibration observation.
    CalibrationSample(CalibrationSample),
    /// A requested medium finished loading.
    MediaReady(MediaId),
    /// An external component failed.
    Error(EngineError),
}

/// A completed trial record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrialRecord {
    /// Trial index within the session.
    pub index: usize,
    /// Session time at which the trial started.
    pub started_at: SessionTime,
    /// Session time at which the trial ended, if it has.
    pub ended_at: Option<SessionTime>,
}

/// A per-frame record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameRecord {
    /// Session time of the frame.
    pub at: SessionTime,
    /// Active trial index.
    pub trial_index: usize,
    /// Predicted gaze, when available.
    pub gaze: Option<GazePoint>,
}

/// A notification for the experimenter console.
#[derive(Clone, Debug, PartialEq)]
pub enum ConsoleEvent {
    /// The lifecycle phase changed.
    PhaseChanged(Phase),
    /// A clock tick occurred.
    Tick {
        /// Session time of the tick.
        at: SessionTime,
    },
    /// A key was not consumed by the engine.
    Key(Key),
    /// A calibration sample was received.
    CalibrationSample(CalibrationSample),
    /// A medium finished loading.
    MediaReady(MediaId),
    /// A non-fatal warning.
    Warning(String),
}

/// An action the application must perform after a state transition.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Present this scene.
    Present(Scene),
    /// Change presenter state.
    Presenter(PresenterCommand),
    /// Append a trial record.
    AppendTrial(TrialRecord),
    /// Append a frame record.
    AppendFrame(FrameRecord),
    /// Notify the console.
    NotifyConsole(ConsoleEvent),
    /// Report an engine error.
    Error(EngineError),
}

/// Apply one event to `state` and return the resulting effects.
///
/// `now` is monotonic session time. The function is pure with respect to
/// everything except `state`; it never reads a clock, sleeps, or performs I/O.
pub fn step(state: &mut EngineState, event: EngineEvent, now: SessionTime) -> Vec<Effect> {
    match event {
        EngineEvent::Start => start(state, now),
        EngineEvent::Pause => pause(state, now),
        EngineEvent::Resume => resume(state, now),
        EngineEvent::Abort => abort(state, now),
        EngineEvent::Tick => tick(state, now),
        EngineEvent::Key(Key::Escape) => abort(state, now),
        EngineEvent::Key(key) => notify(ConsoleEvent::Key(key)),
        EngineEvent::CalibrationSample(sample) => notify(ConsoleEvent::CalibrationSample(sample)),
        EngineEvent::MediaReady(media) => notify(ConsoleEvent::MediaReady(media)),
        EngineEvent::Error(error) => fail(state, error),
    }
}

fn start(state: &mut EngineState, now: SessionTime) -> Vec<Effect> {
    match state.phase {
        Phase::Idle => {
            state.phase = Phase::Running;
            state.trial_index = 0;
            state.session_start = Some(now);
            state.last_tick = Some(now);
            state.paused_at = None;
            state.paused_total = Duration::ZERO;

            vec![
                Effect::NotifyConsole(ConsoleEvent::PhaseChanged(Phase::Running)),
                Effect::AppendTrial(TrialRecord {
                    index: 0,
                    started_at: now,
                    ended_at: None,
                }),
                Effect::Present(Scene::new()),
            ]
        }
        Phase::Running | Phase::Paused => {
            notify(ConsoleEvent::Warning("engine is already running".into()))
        }
        Phase::Finished | Phase::Aborted => invalid(state.phase, "start"),
    }
}

fn pause(state: &mut EngineState, now: SessionTime) -> Vec<Effect> {
    match state.phase {
        Phase::Running => {
            state.phase = Phase::Paused;
            state.paused_at = Some(now);
            notify(ConsoleEvent::PhaseChanged(Phase::Paused))
        }
        Phase::Paused => notify(ConsoleEvent::Warning("engine is already paused".into())),
        Phase::Idle | Phase::Finished | Phase::Aborted => invalid(state.phase, "pause"),
    }
}

fn resume(state: &mut EngineState, now: SessionTime) -> Vec<Effect> {
    match state.phase {
        Phase::Paused => {
            if let Some(paused_at) = state.paused_at.take() {
                state.paused_total = state
                    .paused_total
                    .saturating_add(now.duration_since(paused_at));
            }
            state.phase = Phase::Running;
            notify(ConsoleEvent::PhaseChanged(Phase::Running))
        }
        Phase::Running => notify(ConsoleEvent::Warning("engine is not paused".into())),
        Phase::Idle | Phase::Finished | Phase::Aborted => invalid(state.phase, "resume"),
    }
}

fn abort(state: &mut EngineState, now: SessionTime) -> Vec<Effect> {
    match state.phase {
        Phase::Idle => {
            state.phase = Phase::Aborted;
            notify(ConsoleEvent::PhaseChanged(Phase::Aborted))
        }
        Phase::Running | Phase::Paused => {
            state.phase = Phase::Aborted;
            state.paused_at = None;
            state.last_tick = Some(now);
            vec![
                Effect::NotifyConsole(ConsoleEvent::PhaseChanged(Phase::Aborted)),
                Effect::Presenter(PresenterCommand::Stop),
            ]
        }
        Phase::Finished | Phase::Aborted => {
            notify(ConsoleEvent::Warning("engine is already inactive".into()))
        }
    }
}

fn tick(state: &mut EngineState, now: SessionTime) -> Vec<Effect> {
    match state.phase {
        Phase::Running | Phase::Paused => {
            state.last_tick = Some(now);
            notify(ConsoleEvent::Tick { at: now })
        }
        Phase::Idle | Phase::Finished | Phase::Aborted => Vec::new(),
    }
}

fn fail(state: &mut EngineState, error: EngineError) -> Vec<Effect> {
    if matches!(state.phase, Phase::Running | Phase::Paused) {
        state.phase = Phase::Aborted;
        state.paused_at = None;
        vec![
            Effect::Error(error),
            Effect::NotifyConsole(ConsoleEvent::PhaseChanged(Phase::Aborted)),
            Effect::Presenter(PresenterCommand::Stop),
        ]
    } else {
        vec![Effect::Error(error)]
    }
}

fn notify(event: ConsoleEvent) -> Vec<Effect> {
    vec![Effect::NotifyConsole(event)]
}

fn invalid(phase: Phase, event: &'static str) -> Vec<Effect> {
    vec![Effect::Error(EngineError::InvalidTransition {
        phase,
        event,
    })]
}

// SPDX-License-Identifier: GPL-3.0-only

//! The pure experiment engine.
//!
//! [`step`] is the only state transition. It never sleeps, performs I/O, or
//! touches a UI. The application owns the loop and executes the returned
//! [`Effect`]s. This is the replacement for Yex's blocking `run()` methods.

use std::time::Duration;

use thiserror::Error;
use yet_present::{MediaId, PresenterCommand, Scene, SessionTime};

use crate::experiment::{BlockId, Stimulus, StimulusId, Trial};
use crate::plan::{PlannedBlock, SessionPlan};
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
    /// Ordered session plan, when the engine is driving a real experiment.
    pub plan: Option<SessionPlan>,
    /// Index of the active block.
    pub block_index: usize,
    /// Index of the active trial within the active block.
    pub trial_index: usize,
    /// Session time at which the session started.
    pub session_start: Option<SessionTime>,
    /// Session time of the most recent tick.
    pub last_tick: Option<SessionTime>,
    /// Session time at which the current pause began.
    pub paused_at: Option<SessionTime>,
    /// Total time spent paused during this session.
    pub paused_total: Duration,
    /// Session time at which the active trial started.
    pub trial_started_at: Option<SessionTime>,
    /// Absolute deadline of the active trial.
    pub deadline: Option<SessionTime>,
}

impl EngineState {
    /// Create an engine state that will drive `plan` on `Start`.
    ///
    /// The plan must contain at least one trial; otherwise `Start` reports
    /// [`EngineError::EmptyPlan`].
    pub fn new(plan: SessionPlan) -> Self {
        Self {
            plan: Some(plan),
            ..Self::default()
        }
    }

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

    /// The active planned block.
    pub fn current_block(&self) -> Option<&PlannedBlock> {
        self.plan.as_ref()?.blocks.get(self.block_index)
    }

    /// The active trial.
    pub fn current_trial(&self) -> Option<&Trial> {
        self.current_block()?
            .trials
            .get(self.trial_index)
            .map(|p| &p.trial)
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
    /// A session plan contains no trials.
    #[error("the session plan contains no trials")]
    EmptyPlan,
    /// The plan and the active indices are inconsistent.
    #[error("the session plan has no active trial")]
    NoActiveTrial,
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

/// A completed or aborted trial record.
#[derive(Clone, Debug, PartialEq)]
pub struct TrialRecord {
    /// Block containing the trial.
    pub block: BlockId,
    /// Trial index within the block.
    pub trial_index: usize,
    /// Stimulus that was presented.
    pub stimulus: StimulusId,
    /// Session time at which the trial started.
    pub started_at: SessionTime,
    /// Session time at which the trial ended.
    pub ended_at: SessionTime,
    /// Whether the trial reached its planned deadline.
    pub completed: bool,
    /// How late the completing tick was, relative to the deadline.
    pub late_by: Duration,
}

/// A per-frame record.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameRecord {
    /// Session time of the frame.
    pub at: SessionTime,
    /// Active block.
    pub block: BlockId,
    /// Active trial index within the block.
    pub trial_index: usize,
    /// Active stimulus.
    pub stimulus: StimulusId,
    /// Session time at which the active trial started.
    pub trial_started_at: SessionTime,
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
    /// A new trial starts; the app should present its stimulus.
    TrialStarted {
        /// Block containing the trial.
        block: BlockId,
        /// Trial index within the block.
        trial_index: usize,
        /// Stimulus to present.
        stimulus: Stimulus,
    },
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
///
/// With a plan, each trial has an absolute deadline. A late tick completes the
/// trial at the tick time, records the lateness, and starts the next trial at
/// that time. This avoids accumulating `dt` error while never silently skipping
/// a trial.
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
        EngineEvent::Error(error) => fail(state, error, now),
    }
}

fn start(state: &mut EngineState, now: SessionTime) -> Vec<Effect> {
    match state.phase {
        Phase::Idle => {
            if let Some(plan) = &state.plan {
                if plan.trial_count() == 0 {
                    return vec![Effect::Error(EngineError::EmptyPlan)];
                }
            }

            state.phase = Phase::Running;
            state.block_index = 0;
            state.trial_index = 0;
            state.session_start = Some(now);
            state.last_tick = Some(now);
            state.paused_at = None;
            state.paused_total = Duration::ZERO;

            if state.plan.is_some() {
                if let Some((block, trial)) = first_trial(state) {
                    state.trial_started_at = Some(now);
                    state.deadline = Some(now.saturating_add(trial.stimulus.duration));
                    vec![
                        Effect::NotifyConsole(ConsoleEvent::PhaseChanged(Phase::Running)),
                        Effect::TrialStarted {
                            block,
                            trial_index: 0,
                            stimulus: trial.stimulus.clone(),
                        },
                    ]
                } else {
                    state.phase = Phase::Idle;
                    vec![Effect::Error(EngineError::EmptyPlan)]
                }
            } else {
                state.trial_started_at = None;
                state.deadline = None;
                vec![
                    Effect::NotifyConsole(ConsoleEvent::PhaseChanged(Phase::Running)),
                    Effect::Present(Scene::new()),
                ]
            }
        }
        Phase::Running | Phase::Paused => {
            notify(ConsoleEvent::Warning("engine is already running".into()))
        }
        Phase::Finished | Phase::Aborted => invalid(state.phase, "start"),
    }
}

fn first_trial(state: &EngineState) -> Option<(BlockId, Trial)> {
    let plan = state.plan.as_ref()?;
    let block = plan.blocks.first()?;
    let trial = block.trials.first()?;
    Some((block.id.clone(), trial.trial.clone()))
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
                let pause_duration = now.duration_since(paused_at);
                state.paused_total = state.paused_total.saturating_add(pause_duration);
                if let Some(deadline) = state.deadline {
                    state.deadline = Some(deadline.saturating_add(pause_duration));
                }
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
            let mut effects = Vec::new();
            if state.plan.is_some() {
                if let Some(record) = current_trial_record(state, now, false, Duration::ZERO) {
                    effects.push(Effect::AppendTrial(record));
                }
            }
            state.phase = Phase::Aborted;
            state.paused_at = None;
            state.last_tick = Some(now);
            state.deadline = None;
            state.trial_started_at = None;
            effects.push(Effect::NotifyConsole(ConsoleEvent::PhaseChanged(
                Phase::Aborted,
            )));
            effects.push(Effect::Presenter(PresenterCommand::Stop));
            effects
        }
        Phase::Finished | Phase::Aborted => {
            notify(ConsoleEvent::Warning("engine is already inactive".into()))
        }
    }
}

fn tick(state: &mut EngineState, now: SessionTime) -> Vec<Effect> {
    match state.phase {
        Phase::Running => {
            state.last_tick = Some(now);
            let mut effects = notify(ConsoleEvent::Tick { at: now });

            if state.plan.is_some() {
                let Some(record) = frame_record(state, now) else {
                    return vec![Effect::Error(EngineError::NoActiveTrial)];
                };
                effects.insert(0, Effect::AppendFrame(record));

                if let Some(deadline) = state.deadline {
                    if now >= deadline {
                        let late_by = now.duration_since(deadline);
                        effects.extend(finish_trial(state, now, late_by));
                    }
                }
            }
            effects
        }
        Phase::Paused => {
            state.last_tick = Some(now);
            notify(ConsoleEvent::Tick { at: now })
        }
        Phase::Idle | Phase::Finished | Phase::Aborted => Vec::new(),
    }
}

fn finish_trial(state: &mut EngineState, ended_at: SessionTime, late_by: Duration) -> Vec<Effect> {
    let Some(record) = current_trial_record(state, ended_at, true, late_by) else {
        return vec![Effect::Error(EngineError::NoActiveTrial)];
    };
    let mut effects = vec![Effect::AppendTrial(record)];

    let Some(plan) = state.plan.as_ref() else {
        state.phase = Phase::Finished;
        state.deadline = None;
        state.trial_started_at = None;
        effects.push(Effect::NotifyConsole(ConsoleEvent::PhaseChanged(
            Phase::Finished,
        )));
        effects.push(Effect::Presenter(PresenterCommand::Stop));
        return effects;
    };

    let block = &plan.blocks[state.block_index];
    if state.trial_index + 1 < block.trials.len() {
        state.trial_index += 1;
    } else if state.block_index + 1 < plan.blocks.len() {
        state.block_index += 1;
        state.trial_index = 0;
    } else {
        state.phase = Phase::Finished;
        state.deadline = None;
        state.trial_started_at = None;
        effects.push(Effect::NotifyConsole(ConsoleEvent::PhaseChanged(
            Phase::Finished,
        )));
        effects.push(Effect::Presenter(PresenterCommand::Stop));
        return effects;
    }

    let Some(block) = state
        .plan
        .as_ref()
        .and_then(|p| p.blocks.get(state.block_index))
    else {
        return vec![Effect::Error(EngineError::NoActiveTrial)];
    };
    let Some(planned) = block.trials.get(state.trial_index) else {
        return vec![Effect::Error(EngineError::NoActiveTrial)];
    };

    let stimulus = planned.trial.stimulus.clone();
    state.trial_started_at = Some(ended_at);
    state.deadline = Some(ended_at.saturating_add(stimulus.duration));
    effects.push(Effect::TrialStarted {
        block: block.id.clone(),
        trial_index: state.trial_index,
        stimulus,
    });
    effects
}

fn current_trial_record(
    state: &EngineState,
    ended_at: SessionTime,
    completed: bool,
    late_by: Duration,
) -> Option<TrialRecord> {
    let block = state.current_block()?;
    let trial = block.trials.get(state.trial_index)?;
    Some(TrialRecord {
        block: block.id.clone(),
        trial_index: state.trial_index,
        stimulus: trial.trial.stimulus.id.clone(),
        started_at: state.trial_started_at.unwrap_or(ended_at),
        ended_at,
        completed,
        late_by,
    })
}

fn frame_record(state: &EngineState, at: SessionTime) -> Option<FrameRecord> {
    let block = state.current_block()?;
    let trial = block.trials.get(state.trial_index)?;
    Some(FrameRecord {
        at,
        block: block.id.clone(),
        trial_index: state.trial_index,
        stimulus: trial.trial.stimulus.id.clone(),
        trial_started_at: state.trial_started_at.unwrap_or(at),
        gaze: None,
    })
}

fn fail(state: &mut EngineState, error: EngineError, now: SessionTime) -> Vec<Effect> {
    if matches!(state.phase, Phase::Running | Phase::Paused) {
        let mut effects = abort(state, now);
        effects.insert(0, Effect::Error(error));
        effects
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

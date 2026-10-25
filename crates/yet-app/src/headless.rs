// SPDX-License-Identifier: GPL-3.0-only

//! A windowless experiment run.
//!
//! The runner drives [`EngineState`] with a synthetic monotonic clock. There
//! is no sleeping: each tick jumps to the next frame step or the active trial
//! deadline. This is exact for an experiment without a camera, and it makes a
//! full demo run finish in milliseconds.
//!
//! A headless run uses a [`RecordingPresenter`], so it exercises the same
//! effect handling and scene composition as the graphical console without
//! opening a window.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use yet_core::layout::StimulusPlacement;
use yet_core::{
    step, Clock, EngineError, EngineEvent, EngineState, FrameRecord, GazeSample, Phase,
    SessionPlan, TestClock,
};
use yet_present::{Presenter, RecordingPresenter, ScreenGeometry};

use crate::driver::{prepare, SessionRunner};

pub use crate::driver::{RunError, RunReport};

/// Simulated frame step used by the headless clock.
pub const TICK: Duration = Duration::from_millis(10);

/// Options for [`run`].
#[derive(Clone, Copy, Debug)]
pub struct HeadlessOptions<'a> {
    /// Experiment directory containing `config.toml` and the CSVs.
    pub experiment_dir: &'a Path,
    /// Participant identifier; defaults to the Unix time at session start.
    pub participant: Option<&'a str>,
    /// Result CSV path; defaults to the experiment directory.
    pub output: Option<&'a Path>,
}

impl<'a> HeadlessOptions<'a> {
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

/// Run an experiment directory end to end and write its result CSV.
pub fn run(options: HeadlessOptions<'_>) -> Result<RunReport, RunError> {
    let crate::driver::Prepared {
        loaded,
        mut sink,
        result_file,
    } = prepare(options.experiment_dir, options.participant, options.output)?;
    let (width, height) = (loaded.config.display.width, loaded.config.display.height);
    let mut presenter = RecordingPresenter::new(ScreenGeometry::new(
        loaded.config.display.presenter_monitor,
        None,
        (width, height),
        1.0,
    ));

    let mut report = drive(loaded.plan, &mut sink, &mut presenter, |_, _| None)?;
    sink.flush()?;
    report.result_file = Some(result_file);
    Ok(report)
}

/// Drive a plan to completion with a synthetic clock.
///
/// `gaze` maps a frame record to a gaze sample. The headless CLI passes a
/// closure returning `None`; tests and the camera adapter can inject samples
/// without changing the scheduler.
pub fn drive<W, P, G>(
    plan: SessionPlan,
    sink: &mut yet_core::DataSink<W>,
    presenter: &mut P,
    gaze: G,
) -> Result<RunReport, RunError>
where
    W: Write,
    P: Presenter,
    G: FnMut(&FrameRecord, Option<StimulusPlacement>) -> Option<GazeSample>,
{
    let mut state = EngineState::new(plan);
    let mut clock = TestClock::new();
    let trials = state.plan.as_ref().map_or(0, SessionPlan::trial_count);
    let mut runner = SessionRunner::new(sink, presenter, gaze);
    runner.report_mut().trials = trials;

    // Bound the loop by planned experiment time; a zero-duration plan (which
    // validation rejects) would otherwise never advance the deadline.
    let planned: Duration = state
        .plan
        .as_ref()
        .map(|plan| {
            plan.blocks
                .iter()
                .flat_map(|block| &block.trials)
                .map(|planned| planned.trial.stimulus.duration)
                .sum()
        })
        .unwrap_or_default();
    let max_ticks = (planned.as_millis() / TICK.as_millis()) as usize + trials + 16;

    runner.apply_all(step(&mut state, EngineEvent::Start, clock.now()))?;

    let mut ticks = 0;
    while state.phase == Phase::Running {
        ticks += 1;
        if ticks > max_ticks {
            return Err(RunError::Stalled);
        }

        let now = clock.now();
        let deadline = state.deadline.ok_or(EngineError::NoActiveTrial)?;
        let tick_at = (now + TICK).min(deadline);
        if tick_at <= now {
            return Err(RunError::Stalled);
        }
        clock.set(tick_at);

        runner.apply_all(step(&mut state, EngineEvent::Tick, tick_at))?;
    }

    if state.phase != Phase::Finished {
        return Err(RunError::Incomplete(state.phase));
    }
    Ok(runner.finish())
}

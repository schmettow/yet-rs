// SPDX-License-Identifier: GPL-3.0-only

//! A windowless experiment run.
//!
//! The runner drives [`EngineState`] with a synthetic monotonic clock. There
//! is no sleeping: each tick jumps to the next frame step or the active trial
//! deadline. This is exact for an experiment without a camera, and it makes a
//! full demo run finish in milliseconds. Real-time camera capture arrives in
//! Sprint 5, when it becomes meaningful to pace against a wall clock.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use thiserror::Error;
use yet_core::{
    step, Clock, ConsoleEvent, DataError, DataSink, Effect, EngineError, EngineEvent, EngineState,
    FrameRecord, GazeSample, LoadError, Phase, SessionIdentity, SessionPlan, Stimulus, StimulusId,
    StimulusKind, TestClock,
};
use yet_present::{MediaId, Presenter, PresenterCommand, PresenterError, RecordingPresenter};

/// Default prefix of a result file.
pub const RESULT_PREFIX: &str = "yet1";

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

/// Summary of a completed headless run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunReport {
    /// Result CSV, when the run was started through [`run`].
    pub result_file: Option<PathBuf>,
    /// Number of planned trials.
    pub trials: usize,
    /// Number of trials that reached their deadline.
    pub completed: usize,
    /// Number of gaze samples written.
    pub samples: usize,
    /// Number of trials completed after their deadline.
    pub late_trials: usize,
    /// Largest lateness of any trial.
    pub max_late: Duration,
    /// Non-fatal engine warnings.
    pub warnings: Vec<String>,
}

/// Errors from a headless run.
#[derive(Debug, Error)]
pub enum RunError {
    /// The experiment directory is invalid.
    #[error(transparent)]
    Load(#[from] LoadError),
    /// The result file could not be created.
    #[error("could not create `{path}`: {source}")]
    Create {
        /// Attempted path.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// A directory could not be created.
    #[error("could not create directory `{path}`: {source}")]
    CreateDirectory {
        /// Attempted path.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The result CSV could not be written.
    #[error(transparent)]
    Data(#[from] DataError),
    /// The presenter rejected a command.
    #[error(transparent)]
    Presenter(#[from] PresenterError),
    /// The system clock is before the Unix epoch.
    #[error("the system clock is before the Unix epoch")]
    Clock,
    /// The engine reported an error.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// A sequence stimulus has no frame rate.
    #[error("stimulus `{0}` is a sequence without a frame rate")]
    MissingFps(StimulusId),
    /// The engine stopped without finishing.
    #[error("the session stopped in phase {0:?}")]
    Incomplete(Phase),
    /// A tick did not advance experiment time.
    #[error("the session scheduler made no progress")]
    Stalled,
}

/// Run an experiment directory end to end and write its result CSV.
pub fn run(options: HeadlessOptions<'_>) -> Result<RunReport, RunError> {
    let loaded = yet_core::load_experiment(options.experiment_dir)?;
    let epoch_start = now_seconds()?;
    let participant = options
        .participant
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{}", epoch_start as u64));
    let identity = SessionIdentity::new(
        &loaded.experiment.id,
        loaded.experiment.experimenter.as_deref(),
        participant,
    );

    let result_file = options
        .output
        .map(Path::to_path_buf)
        .unwrap_or_else(|| loaded.root.join(identity.file_name(RESULT_PREFIX)));
    if let Some(parent) = result_file.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|source| RunError::CreateDirectory {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }
    let file = File::create(&result_file).map_err(|source| RunError::Create {
        path: result_file.clone(),
        source,
    })?;

    let mut sink = DataSink::new(BufWriter::new(file), identity, epoch_start)?;
    let (width, height) = (loaded.config.display.width, loaded.config.display.height);
    let mut presenter = RecordingPresenter::new(yet_present::ScreenGeometry::new(
        loaded.config.display.presenter_monitor,
        None,
        (width, height),
        1.0,
    ));

    let mut report = drive(loaded.plan, &mut sink, &mut presenter, |_| None)?;
    sink.flush()?;
    report.result_file = Some(result_file);
    Ok(report)
}

/// Drive a plan to completion with a synthetic clock.
///
/// `gaze` maps a frame record to a gaze sample. The headless CLI passes a
/// closure returning `None`; tests and the later camera adapter can inject
/// samples without changing the scheduler.
pub fn drive<W, P, G>(
    plan: SessionPlan,
    sink: &mut DataSink<W>,
    presenter: &mut P,
    gaze: G,
) -> Result<RunReport, RunError>
where
    W: Write,
    P: Presenter,
    G: FnMut(&FrameRecord) -> Option<GazeSample>,
{
    let mut state = EngineState::new(plan);
    let mut clock = TestClock::new();
    let report = RunReport {
        trials: state.plan.as_ref().map_or(0, SessionPlan::trial_count),
        ..RunReport::default()
    };

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
    let max_ticks = (planned.as_millis() / TICK.as_millis()) as usize + report.trials + 16;

    let mut session = Session {
        report,
        sink,
        presenter,
        media_ids: HashMap::new(),
        next_media: 1,
        gaze,
    };

    session.apply_all(step(&mut state, EngineEvent::Start, clock.now()))?;

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

        session.apply_all(step(&mut state, EngineEvent::Tick, tick_at))?;
    }

    if state.phase != Phase::Finished {
        return Err(RunError::Incomplete(state.phase));
    }
    Ok(session.report)
}

struct Session<'a, W: Write, P: Presenter, G> {
    report: RunReport,
    sink: &'a mut DataSink<W>,
    presenter: &'a mut P,
    media_ids: HashMap<StimulusId, MediaId>,
    next_media: u32,
    gaze: G,
}

impl<W, P, G> Session<'_, W, P, G>
where
    W: Write,
    P: Presenter,
    G: FnMut(&FrameRecord) -> Option<GazeSample>,
{
    fn apply_all(&mut self, effects: Vec<Effect>) -> Result<(), RunError> {
        for effect in effects {
            self.apply(effect)?;
        }
        Ok(())
    }

    fn apply(&mut self, effect: Effect) -> Result<(), RunError> {
        match effect {
            Effect::TrialStarted { stimulus, .. } => {
                let media = self.media_id(&stimulus)?;
                self.presenter.apply(PresenterCommand::Play { id: media })?;
            }
            Effect::AppendTrial(record) => {
                self.sink.flush()?;
                if record.completed {
                    self.report.completed += 1;
                }
                if record.late_by > Duration::ZERO {
                    self.report.late_trials += 1;
                    self.report.max_late = self.report.max_late.max(record.late_by);
                }
            }
            Effect::AppendFrame(record) => {
                if let Some(sample) = (self.gaze)(&record) {
                    self.sink.append(&record.stimulus, sample)?;
                    self.report.samples += 1;
                }
            }
            Effect::Presenter(command) => self.presenter.apply(command)?,
            Effect::Present(_scene) => {}
            Effect::NotifyConsole(event) => {
                if let ConsoleEvent::Warning(message) = event {
                    self.report.warnings.push(message);
                }
            }
            Effect::Error(error) => return Err(RunError::Engine(error)),
        }
        Ok(())
    }

    fn media_id(&mut self, stimulus: &Stimulus) -> Result<MediaId, RunError> {
        if let Some(media) = self.media_ids.get(&stimulus.id) {
            return Ok(*media);
        }

        let media = MediaId::new(self.next_media);
        self.next_media += 1;
        let command = match stimulus.kind {
            StimulusKind::Image => PresenterCommand::LoadImage {
                id: media,
                path: stimulus.path.clone(),
            },
            StimulusKind::Sequence => PresenterCommand::LoadSequence {
                id: media,
                dir: stimulus.path.clone(),
                fps: stimulus
                    .fps
                    .ok_or_else(|| RunError::MissingFps(stimulus.id.clone()))?,
                looped: stimulus.looped,
                audio: stimulus.audio.clone(),
            },
        };
        self.presenter.apply(command)?;
        self.media_ids.insert(stimulus.id.clone(), media);
        Ok(media)
    }
}

fn now_seconds() -> Result<f64, RunError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .map_err(|_| RunError::Clock)
}

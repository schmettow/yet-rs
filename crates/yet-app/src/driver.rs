// SPDX-License-Identifier: GPL-3.0-only

//! Shared session driver for headless and graphical runs.
//!
//! The driver executes [`Effect`]s from the pure engine: it loads media into the
//! presenter, composes the stimulus scene in physical pixels, feeds the data
//! sink, and presents frames. It contains no UI code, so the headless runner
//! and the console use the same behaviour.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use thiserror::Error;
use yet_core::layout::StimulusPlacement;
use yet_core::{
    DataError, DataSink, Effect, EngineError, FrameRecord, GazePoint, GazeSample, LoadError,
    LoadedExperiment, Phase, SessionIdentity, Stimulus, StimulusId, StimulusKind,
};
use yet_present::{
    AudioError, ClockSource, FrameStamp, MediaId, PresentStats, Presenter, PresenterCommand,
    PresenterError, Scene, SessionTime,
};

/// Default prefix of a result file.
pub const RESULT_PREFIX: &str = "yet1";

/// Summary of a completed session.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunReport {
    /// Result CSV, when the run was started through a prepared sink.
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

/// Errors from a session run.
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
    /// The audio backend could not be opened.
    #[error(transparent)]
    Audio(#[from] AudioError),
    /// The overlay backend could not be opened.
    #[error(transparent)]
    Overlay(#[from] yet_present::OverlayError),
    /// The system clock is before the Unix epoch.
    #[error("the system clock is before the Unix epoch")]
    Clock,
    /// The engine reported an error.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// A sequence stimulus has no frame rate.
    #[error("stimulus `{0}` is a sequence without a frame rate")]
    MissingFps(StimulusId),
    /// A stimulus cannot be placed on the monitor.
    #[error(
        "stimulus `{id}` is {media_px:?} px and the presenter is {surface_px:?} px; check the display configuration"
    )]
    InvalidPlacement {
        /// Stimulus identifier.
        id: StimulusId,
        /// Natural media size.
        media_px: (u32, u32),
        /// Presenter surface size.
        surface_px: (u32, u32),
    },
    /// The window backend failed.
    #[error("could not open the window: {0}")]
    Window(String),
    /// The session thread could not be started.
    #[error("could not start the session thread: {0}")]
    Thread(String),
    /// The engine stopped without finishing.
    #[error("the session stopped in phase {0:?}")]
    Incomplete(Phase),
    /// A tick did not advance experiment time.
    #[error("the session scheduler made no progress")]
    Stalled,
}

/// An experiment, a result sink, and the result path.
pub struct Prepared {
    /// Loaded experiment configuration and plan.
    pub loaded: LoadedExperiment,
    /// Open result CSV.
    pub sink: DataSink<BufWriter<File>>,
    /// Result CSV path.
    pub result_file: PathBuf,
}

/// Load an experiment and open its result CSV.
pub fn prepare(
    experiment_dir: &Path,
    participant: Option<&str>,
    output: Option<&Path>,
) -> Result<Prepared, RunError> {
    let loaded = yet_core::load_experiment(experiment_dir)?;
    let epoch_start = now_seconds()?;
    let participant = participant
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{}", epoch_start as u64));
    let identity = SessionIdentity::new(
        &loaded.experiment.id,
        loaded.experiment.experimenter.as_deref(),
        participant,
    );

    let result_file = output
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
    let sink = DataSink::new(BufWriter::new(file), identity, epoch_start)?;

    Ok(Prepared {
        loaded,
        sink,
        result_file,
    })
}

/// Executes engine effects against a presenter and a data sink.
pub struct SessionRunner<'a, W: Write, P: Presenter, G> {
    report: RunReport,
    sink: &'a mut DataSink<W>,
    presenter: &'a mut P,
    media_ids: HashMap<StimulusId, MediaId>,
    next_media: u32,
    gaze: G,
    scene: Scene,
    placement: Option<StimulusPlacement>,
    current: Option<StimulusId>,
    current_media: Option<MediaId>,
}

impl<'a, W, P, G> SessionRunner<'a, W, P, G>
where
    W: Write,
    P: Presenter,
    G: FnMut(&FrameRecord, Option<StimulusPlacement>) -> Option<GazeSample>,
{
    /// Create a runner around an open sink and presenter.
    pub fn new(sink: &'a mut DataSink<W>, presenter: &'a mut P, gaze: G) -> Self {
        Self {
            report: RunReport::default(),
            sink,
            presenter,
            media_ids: HashMap::new(),
            next_media: 1,
            gaze,
            scene: Scene::new(),
            placement: None,
            current: None,
            current_media: None,
        }
    }

    /// The current report.
    pub fn report(&self) -> &RunReport {
        &self.report
    }

    /// Mutable access to the report, for pre-run counters.
    pub fn report_mut(&mut self) -> &mut RunReport {
        &mut self.report
    }

    /// The scene that will be presented next.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Placement of the current stimulus, when it was composed.
    pub fn placement(&self) -> Option<StimulusPlacement> {
        self.placement
    }

    /// The active stimulus identifier.
    pub fn current(&self) -> Option<&StimulusId> {
        self.current.as_ref()
    }

    /// Geometry of the monitor this runner presents on.
    pub fn geometry(&self) -> yet_present::ScreenGeometry {
        self.presenter.geometry()
    }

    /// The media identifier of the active stimulus.
    pub fn current_media(&self) -> Option<MediaId> {
        self.current_media
    }

    /// Flush the result sink.
    pub fn flush(&mut self) -> Result<(), RunError> {
        self.sink.flush()?;
        Ok(())
    }

    /// Finish the run and return its report.
    pub fn finish(self) -> RunReport {
        self.report
    }

    /// Apply every effect in order.
    pub fn apply_all(&mut self, effects: Vec<Effect>) -> Result<(), RunError> {
        for effect in effects {
            self.apply(effect)?;
        }
        Ok(())
    }

    /// Apply one effect.
    pub fn apply(&mut self, effect: Effect) -> Result<(), RunError> {
        match effect {
            Effect::TrialStarted { stimulus, .. } => {
                let media = self.media_id(&stimulus)?;
                self.compose_scene(&stimulus, media)?;
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
                if let Some(sample) = (self.gaze)(&record, self.placement) {
                    self.sink.append(&record.stimulus, sample)?;
                    self.report.samples += 1;
                }
            }
            Effect::Presenter(command) => self.presenter.apply(command)?,
            Effect::Present(scene) => {
                self.scene = scene;
                self.placement = None;
                self.current = None;
                self.current_media = None;
            }
            Effect::NotifyConsole(event) => {
                if let yet_core::ConsoleEvent::Warning(message) = event {
                    self.report.warnings.push(message);
                }
            }
            Effect::Error(error) => return Err(RunError::Engine(error)),
        }
        Ok(())
    }

    /// Present the current scene and return its timing statistics.
    pub fn present(&mut self, at: SessionTime, dt: Duration) -> PresentStats {
        self.refresh_placement();
        let frame = frame_stamp(at, dt);
        self.presenter.present(&frame, &self.scene)
    }

    /// Present an arbitrary scene without changing the trial scene.
    ///
    /// Calibration uses this to draw its grid while the experiment scene is
    /// paused or not yet started.
    pub fn present_scene(&mut self, at: SessionTime, dt: Duration, scene: &Scene) -> PresentStats {
        let frame = frame_stamp(at, dt);
        self.presenter.present(&frame, scene)
    }

    /// Recompose the current stimulus after the presenter surface changed.
    ///
    /// The first paint of a new viewport happens before the window manager
    /// applies fullscreen, and a monitor change can resize the surface at any
    /// time. Recomputing the placement keeps the stimulus in the same
    /// monitor-relative position instead of freezing the startup geometry.
    pub fn refresh_placement(&mut self) {
        let Some(media) = self.current_media else {
            return;
        };
        let Some(media_px) = self.presenter.media_size(media) else {
            return;
        };
        let surface_px = self.presenter.geometry().size_px;
        let Some(placement) = StimulusPlacement::fit(media_px, surface_px) else {
            return;
        };
        if self.placement != Some(placement) {
            self.placement = Some(placement);
            self.scene = placement.scene(media);
        }
    }

    fn compose_scene(&mut self, stimulus: &Stimulus, media: MediaId) -> Result<(), RunError> {
        self.current_media = Some(media);
        let Some(media_px) = self.presenter.media_size(media) else {
            self.scene = Scene::new();
            self.placement = None;
            self.current = Some(stimulus.id.clone());
            return Ok(());
        };
        let surface_px = self.presenter.geometry().size_px;
        let Some(placement) = StimulusPlacement::fit(media_px, surface_px) else {
            return Err(RunError::InvalidPlacement {
                id: stimulus.id.clone(),
                media_px,
                surface_px,
            });
        };

        self.placement = Some(placement);
        self.scene = placement.scene(media);
        self.current = Some(stimulus.id.clone());
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

/// Map a monitor-space gaze point to stimulus pixels and normalized units.
///
/// The placement is the fitted rectangle reported by [`StimulusPlacement`];
/// points outside the stimulus are not clamped, so a tracker excursion stays
/// visible in the CSV.
pub fn gaze_sample(
    record: &FrameRecord,
    monitor: GazePoint,
    placement: StimulusPlacement,
) -> GazeSample {
    let scale = placement.scale.max(f32::EPSILON);
    let dest = placement.dest_px;
    let pixel = GazePoint::new((monitor.x - dest.x) / scale, (monitor.y - dest.y) / scale);
    let media_width = (dest.width / scale).max(f32::EPSILON);
    let media_height = (dest.height / scale).max(f32::EPSILON);
    let normalized = GazePoint::new(pixel.x / media_width, pixel.y / media_height);
    GazeSample::new(record.at, pixel, normalized)
}

pub(crate) fn now_seconds() -> Result<f64, RunError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .map_err(|_| RunError::Clock)
}

fn frame_stamp(at: SessionTime, dt: Duration) -> FrameStamp {
    let now = Instant::now();
    FrameStamp {
        experiment_t: at,
        requested_at: now,
        presented_at: now,
        dt,
        media_pts: None,
        audio_pts: None,
        master: ClockSource::Monotonic,
        dropped: false,
    }
}

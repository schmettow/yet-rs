// SPDX-License-Identifier: GPL-3.0-only

//! Presentation commands, statistics, and backend traits.
//!
//! This module is the contract between the UI-free engine and any presenter,
//! overlay, media source, or audio backend. It contains no toolkit types.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::geometry::ScreenGeometry;
use crate::scene::{MediaId, Scene};
use crate::time::SessionTime;

/// Clock used to pace a presented frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockSource {
    /// The audio clock is authoritative.
    Audio,
    /// The monotonic session clock is authoritative.
    Monotonic,
}

/// Timing information for one presented frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameStamp {
    /// Experiment-relative time at which the frame belongs.
    pub experiment_t: SessionTime,
    /// When the presenter was asked to present the frame.
    pub requested_at: Instant,
    /// When presentation was observed or is estimated to have completed.
    pub presented_at: Instant,
    /// Time since the previous frame.
    pub dt: Duration,
    /// Media presentation timestamp, when the frame belongs to a sequence.
    pub media_pts: Option<Duration>,
    /// Audio clock position, when the frame is audio-paced.
    pub audio_pts: Option<Duration>,
    /// Clock that determined the frame.
    pub master: ClockSource,
    /// Whether the frame was late and dropped, or replaced an unpainted frame.
    pub dropped: bool,
}

/// Statistics returned for a presented frame.
#[derive(Clone, Debug, PartialEq)]
pub struct PresentStats {
    /// Timing information for the frame.
    pub frame: FrameStamp,
    /// Number of frames dropped since the previous report.
    pub dropped_frames: u32,
    /// Audio/video drift in milliseconds, when audio is present.
    pub audio_drift_ms: Option<f32>,
    /// Number of audio underruns since the previous report.
    pub audio_underruns: u32,
}

/// A command that changes presenter or playback state.
#[derive(Clone, Debug, PartialEq)]
pub enum PresenterCommand {
    /// Load a still image and associate it with `id`.
    LoadImage {
        /// Identifier used by subsequent scenes.
        id: MediaId,
        /// Path to the image file.
        path: PathBuf,
    },
    /// Load a pre-decoded frame sequence and optional WAV track.
    LoadSequence {
        /// Identifier used by subsequent scenes.
        id: MediaId,
        /// Directory containing the frames.
        dir: PathBuf,
        /// Frames per second.
        fps: f32,
        /// Whether playback loops.
        looped: bool,
        /// Optional WAV track.
        audio: Option<PathBuf>,
    },
    /// Show the presenter on a monitor.
    ShowMonitor {
        /// Zero-based monitor index.
        monitor: usize,
        /// Whether to use borderless fullscreen.
        fullscreen: bool,
    },
    /// Start playback of a loaded medium.
    Play {
        /// Media to play.
        id: MediaId,
    },
    /// Pause playback and the media clock.
    Pause,
    /// Resume playback.
    Resume,
    /// Stop playback and clear the active sequence.
    Stop,
    /// Set output volume in `0.0..=1.0`.
    SetVolume(f32),
    /// Release presenter resources.
    Shutdown,
}

/// A frame returned by a media sequence source.
///
/// The frame identifies the media and its presentation timestamp. The pixel
/// buffer stays in the media store; protocol types never carry image data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Media sequence the frame belongs to.
    pub media: MediaId,
    /// Presentation timestamp relative to the start of the sequence.
    pub pts: Duration,
}

/// Errors returned by a presenter.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PresenterError {
    /// The presenter does not know the media identifier.
    #[error("unknown media id {0:?}")]
    UnknownMedia(MediaId),
    /// The presenter has already been shut down.
    #[error("presenter has been shut down")]
    ShutDown,
    /// Media could not be decoded or read.
    #[error("could not load media `{path}`: {message}")]
    Media {
        /// File or directory that failed.
        path: PathBuf,
        /// Decoder or I/O message.
        message: String,
    },
    /// The command is not valid for this presenter.
    #[error("invalid presenter command: {0}")]
    InvalidCommand(String),
    /// Audio playback or device setup failed.
    #[error("audio: {0}")]
    Audio(String),
}

/// Errors returned by an overlay backend.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum OverlayError {
    /// No usable overlay backend exists on this platform/compositor.
    #[error("overlay backend is unavailable: {0}")]
    Unavailable(String),
    /// The requested monitor does not exist.
    #[error("monitor {0} does not exist")]
    UnknownMonitor(usize),
    /// The backend cannot render this command.
    #[error("overlay cannot draw this command: {0}")]
    Unsupported(String),
    /// The overlay could not render a frame.
    #[error("overlay render failed: {0}")]
    Render(String),
}

/// Errors returned by a media sequence source.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum MediaError {
    /// The media file or directory does not exist.
    #[error("media file not found: {0}")]
    NotFound(PathBuf),
    /// The media format is not supported.
    #[error("media format is not supported: {0}")]
    Unsupported(String),
    /// A media file could not be decoded.
    #[error("could not decode `{path}`: {message}")]
    Decode {
        /// File that failed.
        path: PathBuf,
        /// Decoder message.
        message: String,
    },
    /// The requested medium has not been loaded.
    #[error("media is not loaded: {0:?}")]
    NotLoaded(MediaId),
}

/// Errors returned by an audio backend.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AudioError {
    /// The output device is unavailable.
    #[error("audio device is unavailable: {0}")]
    DeviceUnavailable(String),
    /// The audio file does not exist.
    #[error("audio file not found: {0}")]
    NotFound(PathBuf),
    /// Playback failed.
    #[error("audio playback failed: {0}")]
    Playback(String),
}

/// Renders scenes on a monitor and owns the media it presents.
///
/// The presenter never advances experiment state. It applies commands and
/// presents the scene it is handed.
pub trait Presenter {
    /// Geometry of the monitor this presenter targets.
    fn geometry(&self) -> ScreenGeometry;

    /// Natural size of loaded media in physical pixels.
    ///
    /// The scene composer uses this to fit and center a stimulus. It returns
    /// `None` while the media is unknown, so a presenter that loads
    /// asynchronously can answer after the load completes.
    fn media_size(&self, media: MediaId) -> Option<(u32, u32)>;

    /// Apply a state-changing command.
    fn apply(&mut self, command: PresenterCommand) -> Result<(), PresenterError>;

    /// Present one frame and return its timing statistics.
    fn present(&mut self, frame: &FrameStamp, scene: &Scene) -> PresentStats;
}

/// A platform overlay backend.
pub trait OverlayBackend {
    /// Show the overlay on a monitor.
    fn show(&mut self, monitor: usize) -> Result<(), OverlayError>;

    /// Hide or show the overlay without releasing it.
    fn set_visible(&mut self, visible: bool);

    /// Enable or disable click-through behavior.
    fn set_passthrough(&mut self, enabled: bool) -> Result<(), OverlayError>;

    /// Submit one transparent overlay scene.
    ///
    /// Scenes are expected to clear to [`crate::Color::TRANSPARENT`] and only
    /// use primitives the backend supports. A backend that cannot draw a
    /// command returns [`OverlayError::Unsupported`] rather than dropping it
    /// silently.
    fn present(&mut self, scene: &Scene) -> Result<(), OverlayError>;
}

/// A source of pre-decoded media frames, such as a PNG sequence.
///
/// This is the media-sequence trait. Camera capture is a separate
/// `CameraSource` in `yet-core`; the two must not be conflated.
pub trait FrameSource {
    /// Preload the first frame and validate the sequence.
    fn preload(&mut self) -> Result<(), MediaError>;

    /// Return the frame for a presentation timestamp.
    ///
    /// `None` means no frame is available yet; an error is reported through
    /// [`Self::preload`] or the implementation's own status.
    fn frame_at(&mut self, t: Duration) -> Option<Frame>;
}

/// A swappable audio playback backend.
pub trait AudioPlayer: Send {
    /// Load a WAV file.
    fn load(&mut self, path: &Path) -> Result<(), AudioError>;

    /// Start or restart playback.
    fn play(&mut self) -> Result<(), AudioError>;

    /// Pause playback.
    fn pause(&mut self);

    /// Resume playback.
    fn resume(&mut self);

    /// Stop playback and reset the position.
    fn stop(&mut self);

    /// Set output volume in `0.0..=1.0`.
    fn set_volume(&mut self, volume: f32);

    /// Current playback position.
    fn position(&self) -> Duration;

    /// Number of buffered-audio underruns observed, for diagnostics.
    ///
    /// Backends that cannot observe underruns return zero. The presenter
    /// reports `1` while an underrun is active and `0` once audio resumes.
    fn underruns(&self) -> u32 {
        0
    }

    /// Hardware output latency to add to [`Self::position`] for the clock the
    /// listener actually hears. Backends without a measured value return zero.
    fn output_latency(&self) -> Duration {
        Duration::ZERO
    }

    /// Whether this backend can advance an audio clock.
    ///
    /// `NullAudioPlayer` returns `false`, so the presenter falls back to the
    /// monotonic clock even when a sequence carries a track.
    fn is_silent(&self) -> bool {
        false
    }

    /// Override the measured output latency. Backends that cannot change it
    /// ignore the request.
    fn set_output_latency(&mut self, _latency: Duration) {}
}

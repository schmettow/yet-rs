// SPDX-License-Identifier: GPL-3.0-only

//! Sequence playback and the audio master clock.
//!
//! Playback state lives on the controller thread. Decoding a frame happens
//! before the painting state is locked, so the UI thread never waits for disk
//! I/O. The painting state only receives the newest decoded frame.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use crate::media::{PngSequence, WavAudio};
use crate::{
    AudioError, AudioPlayer, ClockSource, MediaError, MediaId, PresenterError, SessionTime,
};
use image::RgbaImage;

/// One loaded PNG sequence and its optional track.
pub(crate) struct SequenceEntry {
    source: PngSequence,
    looped: bool,
    audio: Option<WavAudio>,
    audio_path: Option<PathBuf>,
}

/// A newly decoded frame that the presenter must show.
pub struct Advance {
    pub media: MediaId,
    pub image: RgbaImage,
    pub media_pts: Duration,
    pub audio_pts: Option<Duration>,
    pub master: ClockSource,
    pub dropped: u32,
    pub drift_ms: Option<f32>,
}

struct Active {
    media: MediaId,
    started: Option<SessionTime>,
    index: usize,
    paused: bool,
}

/// All loaded sequences and the playing one.
pub struct PlaybackBank {
    sequences: HashMap<MediaId, SequenceEntry>,
    active: Option<Active>,
    audio: Box<dyn AudioPlayer>,
    audio_master: bool,
    volume: f32,
    dropped: u32,
    underrun_state: bool,
}

impl PlaybackBank {
    pub fn new(audio: Box<dyn AudioPlayer>, volume: f32) -> Self {
        let audio_master = !audio.is_silent();
        Self {
            sequences: HashMap::new(),
            active: None,
            audio,
            audio_master,
            volume,
            dropped: 0,
            underrun_state: false,
        }
    }

    /// Load and validate a sequence, returning its first frame.
    pub fn load_sequence(
        &mut self,
        id: MediaId,
        dir: PathBuf,
        fps: f32,
        looped: bool,
        audio_path: Option<PathBuf>,
    ) -> Result<RgbaImage, PresenterError> {
        let source = PngSequence::open(&dir, fps).map_err(|error| media_error(&dir, error))?;
        let audio = match &audio_path {
            Some(path) => Some(WavAudio::load(path).map_err(audio_error)?),
            None => None,
        };
        let first = source.decode(0).map_err(|error| media_error(&dir, error))?;

        self.sequences.insert(
            id,
            SequenceEntry {
                source,
                looped,
                audio,
                audio_path,
            },
        );
        Ok(first)
    }

    /// Start a loaded sequence, or stop playback when the medium is an image.
    pub fn play(&mut self, id: MediaId) -> Result<(), PresenterError> {
        let Some(entry) = self.sequences.get(&id) else {
            self.stop();
            return Ok(());
        };
        if self.audio_master {
            if let Some(path) = &entry.audio_path {
                self.audio.load(path).map_err(audio_error)?;
            }
            self.audio.set_volume(self.volume);
            if entry.audio.is_some() {
                self.audio.play().map_err(audio_error)?;
            }
        }
        self.active = Some(Active {
            media: id,
            started: None,
            index: 0,
            paused: false,
        });
        self.dropped = 0;
        self.underrun_state = false;
        Ok(())
    }

    /// Pause sequence playback and audio.
    pub fn pause(&mut self) {
        self.audio.pause();
        if let Some(active) = &mut self.active {
            active.paused = true;
        }
    }

    /// Resume sequence playback and audio.
    pub fn resume(&mut self) {
        self.audio.resume();
        if let Some(active) = &mut self.active {
            active.paused = false;
        }
    }

    /// Stop playback and release the active sequence.
    pub fn stop(&mut self) {
        self.audio.stop();
        self.active = None;
        self.dropped = 0;
        self.underrun_state = false;
    }

    /// Set playback volume.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        self.audio.set_volume(self.volume);
    }

    /// Advance to the frame for the current clock, decoding it when it changed.
    ///
    /// Returns `None` when the frame did not change, playback is paused, or no
    /// sequence is active. Decode failures keep the previous frame and count as
    /// a dropped frame.
    pub fn advance(&mut self, experiment_t: SessionTime) -> Option<Advance> {
        let active = self.active.as_mut()?;
        if active.paused {
            return None;
        }
        let entry = self.sequences.get(&active.media)?;
        let started = *active.started.get_or_insert(experiment_t);
        let monotonic = experiment_t.duration_since(started);

        let (elapsed, audio_pts, master) = if entry.audio.is_some() && self.audio_master {
            let position = self.audio.position() + self.audio.output_latency();
            (position, Some(position), ClockSource::Audio)
        } else {
            (monotonic, None, ClockSource::Monotonic)
        };

        let index = entry.source.frame_index_at(elapsed, entry.looped);
        let drift_ms =
            audio_pts.map(|audio| (audio.as_secs_f64() - monotonic.as_secs_f64()) as f32 * 1000.0);
        if index == active.index {
            return None;
        }

        let dropped = if index > active.index + 1 {
            (index - active.index - 1) as u32
        } else {
            0
        };
        let media_pts = entry.source.pts(index);
        match entry.source.decode(index) {
            Ok(image) => {
                active.index = index;
                self.dropped = self.dropped.saturating_add(dropped);
                Some(Advance {
                    media: active.media,
                    image,
                    media_pts,
                    audio_pts,
                    master,
                    dropped,
                    drift_ms,
                })
            }
            Err(_) => {
                // Keep the last decoded frame; the next tick tries again.
                self.dropped = self.dropped.saturating_add(1);
                None
            }
        }
    }

    /// Whether the audio backend currently reports an underrun.
    ///
    /// The returned value is `1` while the condition is active and `0`
    /// otherwise; the caller accumulates transitions.
    pub fn underrun(&mut self) -> u32 {
        let current = self.audio.underruns() > 0;
        let entered = current && !self.underrun_state;
        self.underrun_state = current;
        u32::from(entered)
    }
}

fn media_error(path: &std::path::Path, error: MediaError) -> PresenterError {
    match error {
        MediaError::NotFound(path) => PresenterError::Media {
            path,
            message: "frame directory not found".to_owned(),
        },
        MediaError::Unsupported(message) => PresenterError::Media {
            path: path.to_path_buf(),
            message,
        },
        MediaError::Decode { path, message } => PresenterError::Media { path, message },
        MediaError::NotLoaded(id) => PresenterError::UnknownMedia(id),
    }
}

fn audio_error(error: AudioError) -> PresenterError {
    PresenterError::Audio(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::Path;
    use std::time::Duration;

    use crate::media::NullAudioPlayer;
    use crate::{AudioError, AudioPlayer, ClockSource, MediaId, SessionTime};
    use image::{Rgba, RgbaImage};

    use super::PlaybackBank;

    struct FakeAudio {
        position: Cell<Duration>,
    }

    impl FakeAudio {
        fn new(position: Duration) -> Self {
            Self {
                position: Cell::new(position),
            }
        }
    }

    impl AudioPlayer for FakeAudio {
        fn load(&mut self, _path: &Path) -> Result<(), AudioError> {
            Ok(())
        }

        fn play(&mut self) -> Result<(), AudioError> {
            Ok(())
        }

        fn pause(&mut self) {}

        fn resume(&mut self) {}

        fn stop(&mut self) {
            self.position.set(Duration::ZERO);
        }

        fn set_volume(&mut self, _volume: f32) {}

        fn position(&self) -> Duration {
            self.position.get()
        }
    }

    fn sequence(directory: &std::path::Path, frames: usize) {
        for index in 1..=frames {
            RgbaImage::from_pixel(2, 2, Rgba([index as u8, 0, 0, 255]))
                .save(directory.join(format!("frame_{index:05}.png")))
                .unwrap();
        }
    }

    fn load(bank: &mut PlaybackBank, directory: &std::path::Path) -> MediaId {
        let media = MediaId::new(1);
        bank.load_sequence(media, directory.to_path_buf(), 10.0, false, None)
            .unwrap();
        media
    }

    #[test]
    fn a_sequence_advances_by_monotonic_time_without_audio() {
        let directory = tempfile::tempdir().unwrap();
        sequence(directory.path(), 4);
        let mut bank = PlaybackBank::new(Box::new(NullAudioPlayer), 1.0);
        let media = load(&mut bank, directory.path());
        bank.play(media).unwrap();

        let start = SessionTime::ZERO;
        assert!(bank.advance(start).is_none());
        let advance = bank.advance(start + Duration::from_millis(100)).unwrap();
        assert_eq!(advance.master, ClockSource::Monotonic);
        assert_eq!(advance.media_pts, Duration::from_millis(100));
        assert_eq!(advance.audio_pts, None);
        assert_eq!(advance.dropped, 0);

        let skipped = bank.advance(start + Duration::from_millis(300)).unwrap();
        assert_eq!(skipped.media_pts, Duration::from_millis(300));
        assert_eq!(skipped.dropped, 1);
    }

    #[test]
    fn audio_is_master_when_a_track_is_present() {
        let directory = tempfile::tempdir().unwrap();
        sequence(directory.path(), 4);
        let audio_path = directory.path().join("audio.wav");
        {
            let spec = hound::WavSpec {
                channels: 2,
                sample_rate: 1_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut writer = hound::WavWriter::create(&audio_path, spec).unwrap();
            for _ in 0..800 {
                writer.write_sample(0_i16).unwrap();
            }
            writer.finalize().unwrap();
        }

        let mut bank = PlaybackBank::new(Box::new(FakeAudio::new(Duration::from_millis(200))), 1.0);
        let media = MediaId::new(2);
        bank.load_sequence(
            media,
            directory.path().to_path_buf(),
            10.0,
            false,
            Some(audio_path),
        )
        .unwrap();
        bank.play(media).unwrap();

        let advance = bank.advance(SessionTime::ZERO).unwrap();

        assert_eq!(advance.master, ClockSource::Audio);
        assert_eq!(advance.media_pts, Duration::from_millis(200));
        assert_eq!(advance.audio_pts, Some(Duration::from_millis(200)));
        assert_eq!(advance.dropped, 1);
        assert_eq!(advance.drift_ms, Some(200.0));
    }

    #[test]
    fn a_silent_backend_uses_the_monotonic_clock_despite_a_track() {
        let directory = tempfile::tempdir().unwrap();
        sequence(directory.path(), 4);
        let audio_path = directory.path().join("audio.wav");
        {
            let spec = hound::WavSpec {
                channels: 2,
                sample_rate: 1_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut writer = hound::WavWriter::create(&audio_path, spec).unwrap();
            for _ in 0..800 {
                writer.write_sample(0_i16).unwrap();
            }
            writer.finalize().unwrap();
        }

        let mut bank = PlaybackBank::new(Box::new(NullAudioPlayer), 1.0);
        let media = MediaId::new(3);
        bank.load_sequence(
            media,
            directory.path().to_path_buf(),
            10.0,
            false,
            Some(audio_path),
        )
        .unwrap();
        bank.play(media).unwrap();
        assert!(bank.advance(SessionTime::ZERO).is_none());

        let advance = bank
            .advance(SessionTime::from_duration(Duration::from_millis(100)))
            .unwrap();

        assert_eq!(advance.master, ClockSource::Monotonic);
        assert_eq!(advance.media_pts, Duration::from_millis(100));
        assert_eq!(advance.audio_pts, None);
    }

    #[test]
    fn pausing_freezes_the_frame() {
        let directory = tempfile::tempdir().unwrap();
        sequence(directory.path(), 3);
        let mut bank = PlaybackBank::new(Box::new(NullAudioPlayer), 1.0);
        let media = load(&mut bank, directory.path());
        bank.play(media).unwrap();
        bank.advance(SessionTime::ZERO);
        bank.pause();

        assert!(bank
            .advance(SessionTime::from_duration(Duration::from_millis(500)))
            .is_none());
    }

    #[test]
    fn an_empty_sequence_is_rejected_before_playback() {
        let directory = tempfile::tempdir().unwrap();
        let mut bank = PlaybackBank::new(Box::new(NullAudioPlayer), 1.0);

        let error = bank
            .load_sequence(
                MediaId::new(1),
                directory.path().to_path_buf(),
                10.0,
                false,
                None,
            )
            .unwrap_err();

        assert!(error.to_string().contains("no PNG"));
    }
}

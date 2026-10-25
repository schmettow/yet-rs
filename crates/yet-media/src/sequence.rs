// SPDX-License-Identifier: GPL-3.0-only

//! Pre-decoded PNG frame sequences.
//!
//! A sequence is a directory of PNG files sorted by file name. The source
//! validates the directory and decodes frames on demand; pacing belongs to the
//! presenter, which knows whether audio or the monotonic clock is master.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use image::RgbaImage;
use yet_present::MediaError;

/// A pre-decoded PNG sequence on disk.
#[derive(Clone, Debug)]
pub struct PngSequence {
    dir: PathBuf,
    files: Vec<PathBuf>,
    fps: f32,
}

/// Metadata returned by [`PngSequence::validate`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SequenceInfo {
    /// Number of PNG frames.
    pub frames: usize,
    /// Playback rate in frames per second.
    pub fps: f32,
    /// Sequence duration in seconds.
    pub duration: Duration,
    /// Size of the first frame in pixels.
    pub size_px: (u32, u32),
}

impl PngSequence {
    /// Scan a directory for PNG frames.
    ///
    /// Frames are sorted by file name, so `frame_00001.png` precedes
    /// `frame_00002.png`. The directory must exist and contain at least one PNG.
    pub fn open(dir: &Path, fps: f32) -> Result<Self, MediaError> {
        if !fps.is_finite() || fps <= 0.0 {
            return Err(MediaError::Unsupported(format!(
                "frame rate must be positive, got {fps}"
            )));
        }
        let entries = fs::read_dir(dir).map_err(|_| MediaError::NotFound(dir.to_path_buf()))?;
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|_| MediaError::NotFound(dir.to_path_buf()))?;
            let path = entry.path();
            let is_png = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("png"));
            if path.is_file() && is_png {
                files.push(path);
            }
        }
        files.sort();
        if files.is_empty() {
            return Err(MediaError::Unsupported(format!(
                "no PNG frames in `{}`",
                dir.display()
            )));
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            files,
            fps,
        })
    }

    /// Directory holding the frames.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Playback rate in frames per second.
    pub const fn fps(&self) -> f32 {
        self.fps
    }

    /// Number of frames.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether the sequence has no frames. Never true for an opened sequence.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Duration at the configured frame rate.
    pub fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.files.len() as f64 / f64::from(self.fps))
    }

    /// Frame index for an elapsed time.
    ///
    /// A looped sequence wraps; otherwise the last frame is repeated.
    pub fn frame_index_at(&self, elapsed: Duration, looped: bool) -> usize {
        let index = (elapsed.as_secs_f64() * f64::from(self.fps)).floor();
        if !index.is_finite() || index <= 0.0 {
            return 0;
        }
        let index = index as usize;
        if looped {
            index % self.files.len()
        } else {
            index.min(self.files.len() - 1)
        }
    }

    /// Presentation timestamp of a frame.
    pub fn pts(&self, index: usize) -> Duration {
        Duration::from_secs_f64(index as f64 / f64::from(self.fps))
    }

    /// Decode one frame.
    pub fn decode(&self, index: usize) -> Result<RgbaImage, MediaError> {
        let path = self
            .files
            .get(index)
            .ok_or_else(|| MediaError::Unsupported(format!("frame {index} is out of range")))?;
        image::open(path)
            .map(|image| image.into_rgba8())
            .map_err(|error| MediaError::Decode {
                path: path.clone(),
                message: error.to_string(),
            })
    }

    /// Decode the first frame and report the sequence metadata.
    pub fn validate(&self) -> Result<SequenceInfo, MediaError> {
        let first = self.decode(0)?;
        Ok(SequenceInfo {
            frames: self.len(),
            fps: self.fps,
            duration: self.duration(),
            size_px: first.dimensions(),
        })
    }
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};
    use yet_present::MediaError;

    use super::PngSequence;

    fn write_frame(directory: &std::path::Path, index: usize, value: u8) {
        RgbaImage::from_pixel(2, 2, Rgba([value, 0, 0, 255]))
            .save(directory.join(format!("frame_{index:05}.png")))
            .unwrap();
    }

    #[test]
    fn frames_are_sorted_and_paced() {
        let directory = tempfile::tempdir().unwrap();
        write_frame(directory.path(), 2, 20);
        write_frame(directory.path(), 1, 10);
        write_frame(directory.path(), 3, 30);

        let sequence = PngSequence::open(directory.path(), 10.0).unwrap();

        assert_eq!(sequence.len(), 3);
        assert_eq!(sequence.frame_index_at(std::time::Duration::ZERO, false), 0);
        assert_eq!(
            sequence.frame_index_at(std::time::Duration::from_millis(150), false),
            1
        );
        assert_eq!(
            sequence.frame_index_at(std::time::Duration::from_secs(1), false),
            2
        );
        assert_eq!(
            sequence.frame_index_at(std::time::Duration::from_millis(300), true),
            0
        );
        assert_eq!(sequence.pts(1), std::time::Duration::from_millis(100));
        assert_eq!(sequence.decode(0).unwrap().get_pixel(0, 0)[0], 10);
        assert_eq!(sequence.decode(2).unwrap().get_pixel(0, 0)[0], 30);
    }

    #[test]
    fn an_empty_directory_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let error = PngSequence::open(directory.path(), 30.0).unwrap_err();

        assert!(matches!(error, MediaError::Unsupported(message) if message.contains("no PNG")));
    }

    #[test]
    fn a_missing_directory_is_rejected() {
        let error = PngSequence::open(std::path::Path::new("/does/not/exist"), 30.0).unwrap_err();

        assert!(matches!(error, MediaError::NotFound { .. }));
    }

    #[test]
    fn a_non_positive_frame_rate_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        write_frame(directory.path(), 1, 1);

        assert!(matches!(
            PngSequence::open(directory.path(), 0.0),
            Err(MediaError::Unsupported(_))
        ));
    }

    #[test]
    fn validation_reports_the_first_frame_size() {
        let directory = tempfile::tempdir().unwrap();
        write_frame(directory.path(), 1, 1);
        write_frame(directory.path(), 2, 2);
        let sequence = PngSequence::open(directory.path(), 2.0).unwrap();

        let info = sequence.validate().unwrap();

        assert_eq!(info.frames, 2);
        assert_eq!(info.size_px, (2, 2));
        assert_eq!(info.duration, std::time::Duration::from_secs(1));
    }
}

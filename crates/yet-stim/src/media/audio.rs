// SPDX-License-Identifier: GPL-3.0-only

//! WAV decoding and rodio playback.
//!
//! WAV is decoded with `hound` into `f32` samples and handed to rodio as a
//! `SamplesBuffer`. Symphonia (and its MPL-2.0 license) is not used. The player
//! reports `get_pos()`; the presenter adds [`DEFAULT_OUTPUT_LATENCY`] because
//! `get_pos()` excludes the hardware output buffer.

use std::num::{NonZeroU16, NonZeroU32};
use std::path::Path;
use std::time::Duration;

use crate::{AudioError, AudioPlayer};
use rodio::cpal::traits::HostTrait;
use rodio::{DeviceSinkBuilder, DeviceTrait, MixerDeviceSink, Player};

/// Measured output-buffer latency of `Player::get_pos()`.
///
/// The integrated S8 measurement showed `get_pos()` tracking wall time, so the
/// default is zero. SPK-4's −128 ms measurement included stream startup and is
/// kept as a reminder to measure per device before claiming tight A/V sync.
/// A configuration value can override this.
pub const DEFAULT_OUTPUT_LATENCY: Duration = Duration::ZERO;

/// A decoded WAV file ready for playback.
#[derive(Clone, Debug)]
pub struct WavAudio {
    samples: Vec<f32>,
    channels: u16,
    sample_rate: u32,
    duration: Duration,
}

impl WavAudio {
    /// Decode a PCM (8/16/32-bit integer or 32-bit float) WAV file.
    pub fn load(path: &Path) -> Result<Self, AudioError> {
        if !path.is_file() {
            return Err(AudioError::NotFound(path.to_path_buf()));
        }
        let reader = hound::WavReader::open(path)
            .map_err(|error| AudioError::Playback(format!("{}: {error}", path.display())))?;
        let spec = reader.spec();
        if spec.channels == 0 || spec.sample_rate == 0 {
            return Err(AudioError::Playback(format!(
                "{}: invalid WAV format",
                path.display()
            )));
        }

        let samples: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
            (hound::SampleFormat::Float, 32) => reader
                .into_samples::<f32>()
                .collect::<Result<_, _>>()
                .map_err(|error| AudioError::Playback(format!("{}: {error}", path.display())))?,
            (hound::SampleFormat::Int, 8) => reader
                .into_samples::<i8>()
                .map(|sample| sample.map(|value| f32::from(value) / 128.0))
                .collect::<Result<_, _>>()
                .map_err(|error| AudioError::Playback(format!("{}: {error}", path.display())))?,
            (hound::SampleFormat::Int, 16) => reader
                .into_samples::<i16>()
                .map(|sample| sample.map(|value| f32::from(value) / 32_768.0))
                .collect::<Result<_, _>>()
                .map_err(|error| AudioError::Playback(format!("{}: {error}", path.display())))?,
            (hound::SampleFormat::Int, 32) => reader
                .into_samples::<i32>()
                .map(|sample| sample.map(|value| value as f32 / 2_147_483_648.0))
                .collect::<Result<_, _>>()
                .map_err(|error| AudioError::Playback(format!("{}: {error}", path.display())))?,
            (format, bits) => {
                return Err(AudioError::Playback(format!(
                    "{}: unsupported WAV format {format:?}/{bits} bits",
                    path.display()
                )));
            }
        };

        if samples.is_empty() {
            return Err(AudioError::Playback(format!(
                "{}: WAV has no samples",
                path.display()
            )));
        }
        let frames = samples.len() / usize::from(spec.channels);
        let duration = Duration::from_secs_f64(frames as f64 / f64::from(spec.sample_rate));

        Ok(Self {
            samples,
            channels: spec.channels,
            sample_rate: spec.sample_rate,
            duration,
        })
    }

    /// Decoded interleaved samples in `-1.0..=1.0`.
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Number of channels.
    pub const fn channels(&self) -> u16 {
        self.channels
    }

    /// Sample rate in hertz.
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Playback duration.
    pub const fn duration(&self) -> Duration {
        self.duration
    }

    fn buffer(&self) -> rodio::buffer::SamplesBuffer {
        rodio::buffer::SamplesBuffer::new(
            NonZeroU16::new(self.channels).expect("validated channel count"),
            NonZeroU32::new(self.sample_rate).expect("validated sample rate"),
            self.samples.clone(),
        )
    }
}

/// A rodio output stream and player, implementing [`AudioPlayer`].
pub struct RodioAudioPlayer {
    _sink: MixerDeviceSink,
    player: Player,
    wav: Option<WavAudio>,
    volume: f32,
    playing: bool,
    latency: Duration,
}

impl RodioAudioPlayer {
    /// Open the default output device or a device matched by name.
    ///
    /// An empty name and `"default"` select the default output. A named device
    /// must appear in `cpal`'s output list; otherwise the error names it.
    pub fn open(device: Option<&str>) -> Result<Self, AudioError> {
        let sink = match device.map(str::trim) {
            None | Some("") | Some("default") => DeviceSinkBuilder::open_default_sink(),
            Some(name) => {
                let host = rodio::cpal::default_host();
                let devices = host.output_devices().map_err(|error| {
                    AudioError::DeviceUnavailable(format!("cannot list devices: {error}"))
                })?;
                let device = devices
                    .into_iter()
                    .find(|device| {
                        device
                            .description()
                            .map(|description| description.name() == name)
                            .unwrap_or(false)
                    })
                    .ok_or_else(|| {
                        AudioError::DeviceUnavailable(format!("no output device named `{name}`"))
                    })?;
                DeviceSinkBuilder::from_device(device)
                    .and_then(|builder| builder.open_sink_or_fallback())
            }
        }
        .map_err(|error| AudioError::DeviceUnavailable(error.to_string()))?;

        let player = Player::connect_new(sink.mixer());
        Ok(Self {
            _sink: sink,
            player,
            wav: None,
            volume: 1.0,
            playing: false,
            latency: DEFAULT_OUTPUT_LATENCY,
        })
    }

    /// Override the measured output latency.
    pub fn set_output_latency(&mut self, latency: Duration) {
        self.latency = latency;
    }

    /// The decoded track, when loaded.
    pub fn wav(&self) -> Option<&WavAudio> {
        self.wav.as_ref()
    }

    /// Duration of the loaded track.
    pub fn duration(&self) -> Option<Duration> {
        self.wav.as_ref().map(WavAudio::duration)
    }
}

impl AudioPlayer for RodioAudioPlayer {
    fn load(&mut self, path: &Path) -> Result<(), AudioError> {
        self.wav = Some(WavAudio::load(path)?);
        Ok(())
    }

    fn play(&mut self) -> Result<(), AudioError> {
        let wav = self
            .wav
            .as_ref()
            .ok_or_else(|| AudioError::Playback("no WAV is loaded".to_owned()))?;
        self.player.stop();
        self.player.append(wav.buffer());
        self.player.set_volume(self.volume);
        self.player.play();
        self.playing = true;
        Ok(())
    }

    fn pause(&mut self) {
        self.player.pause();
        self.playing = false;
    }

    fn resume(&mut self) {
        self.player.play();
        self.playing = true;
    }

    fn stop(&mut self) {
        self.player.stop();
        self.playing = false;
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        self.player.set_volume(self.volume);
    }

    fn position(&self) -> Duration {
        self.player.get_pos()
    }

    fn underruns(&self) -> u32 {
        let duration = self.wav.as_ref().map(WavAudio::duration);
        match duration {
            Some(duration)
                if self.playing
                    && self.player.empty()
                    && self.player.get_pos() + Duration::from_millis(50) < duration =>
            {
                1
            }
            _ => 0,
        }
    }

    fn output_latency(&self) -> Duration {
        self.latency
    }
}

/// An audio player that ignores every request, for headless runs.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullAudioPlayer;

impl AudioPlayer for NullAudioPlayer {
    fn load(&mut self, _path: &Path) -> Result<(), AudioError> {
        Ok(())
    }

    fn play(&mut self) -> Result<(), AudioError> {
        Ok(())
    }

    fn pause(&mut self) {}

    fn resume(&mut self) {}

    fn stop(&mut self) {}

    fn set_volume(&mut self, _volume: f32) {}

    fn position(&self) -> Duration {
        Duration::ZERO
    }

    fn is_silent(&self) -> bool {
        true
    }
}

/// Open a player for the configured device, or a null player when audio is off.
pub fn open_player(enabled: bool, device: &str) -> Result<Box<dyn AudioPlayer>, AudioError> {
    if enabled {
        Ok(Box::new(RodioAudioPlayer::open(Some(device))?))
    } else {
        Ok(Box::new(NullAudioPlayer))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{open_player, WavAudio, DEFAULT_OUTPUT_LATENCY};
    use crate::{AudioError, AudioPlayer};

    #[test]
    fn a_wav_is_decoded_to_normalized_samples_and_duration() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tone.wav");
        let samples = [0_i16, 16_384, -16_384, 32_767];
        {
            let spec = hound::WavSpec {
                channels: 2,
                sample_rate: 2_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut writer = hound::WavWriter::create(&path, spec).unwrap();
            for sample in samples {
                writer.write_sample(sample).unwrap();
            }
            writer.finalize().unwrap();
        }

        let wav = WavAudio::load(&path).unwrap();

        assert_eq!(wav.channels(), 2);
        assert_eq!(wav.sample_rate(), 2_000);
        assert_eq!(wav.duration(), std::time::Duration::from_millis(1));
        assert_eq!(wav.samples(), &[0.0, 0.5, -0.5, 32_767.0 / 32_768.0]);
    }

    #[test]
    fn a_missing_wav_is_reported() {
        assert!(matches!(
            WavAudio::load(Path::new("/does/not/exist.wav")),
            Err(AudioError::NotFound(_))
        ));
    }

    #[test]
    fn a_disabled_audio_backend_is_a_null_player() {
        let mut player = open_player(false, "default").unwrap();

        player.load(Path::new("/does/not/exist.wav")).unwrap();
        player.play().unwrap();
        assert_eq!(player.position(), std::time::Duration::ZERO);
        assert_eq!(player.underruns(), 0);
        assert_eq!(player.output_latency(), std::time::Duration::ZERO);
    }

    #[test]
    fn the_integrated_default_latency_is_zero() {
        assert_eq!(DEFAULT_OUTPUT_LATENCY, std::time::Duration::ZERO);
    }

    #[test]
    fn an_unknown_device_name_is_reported() {
        let error = match super::RodioAudioPlayer::open(Some("yet-no-such-output-device")) {
            Ok(_) => panic!("an unknown device must not open"),
            Err(error) => error,
        };

        assert!(matches!(error, AudioError::DeviceUnavailable(_)));
    }

    #[test]
    #[ignore = "requires an audio output device"]
    fn the_default_device_opens_and_advances() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("silence.wav");
        {
            let spec = hound::WavSpec {
                channels: 2,
                sample_rate: 48_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut writer = hound::WavWriter::create(&path, spec).unwrap();
            for _ in 0..(48_000 * 2 * 2) {
                writer.write_sample(0_i16).unwrap();
            }
            writer.finalize().unwrap();
        }

        let mut player = super::RodioAudioPlayer::open(None).unwrap();
        player.load(&path).unwrap();
        player.play().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(250));

        assert!(
            player.position() > std::time::Duration::from_millis(50),
            "audio clock did not advance: {:?}",
            player.position()
        );
        player.stop();
    }
}

// SPDX-License-Identifier: GPL-3.0-only
//! SPK-4 playback experiment (requires `--features rodio`).
//!
//! This uses `hound` to decode the WAV into `f32` samples and feeds a
//! `rodio::SamplesBuffer` to a `Player`, avoiding Rodio's Symphonia decoder.
//!
//! It is compile-blocked on this machine because `cpal` -> `alsa-sys` cannot
//! find the ALSA development package. Run on a machine with `libasound2-dev`
//! (Debian/Ubuntu) or the equivalent:
//!
//!     cargo run --release --features rodio --bin playback

use std::error::Error;
use std::num::{NonZeroU16, NonZeroU32};
use std::thread::sleep;
use std::time::{Duration, Instant};

use hound::WavReader;
use rodio::buffer::SamplesBuffer;

const FPS: f64 = 30.0;

fn main() -> Result<(), Box<dyn Error>> {
    let reader = WavReader::open("audio.wav")?;
    let spec = reader.spec();
    let samples: Vec<f32> = reader
        .into_samples::<i16>()
        .map(|sample| sample.map(|value| f32::from(value) / 32768.0))
        .collect::<Result<_, _>>()?;

    let channels = NonZeroU16::new(spec.channels).ok_or("WAV has zero channels")?;
    let sample_rate = NonZeroU32::new(spec.sample_rate).ok_or("WAV has zero sample rate")?;

    let device = rodio::DeviceSinkBuilder::open_default_sink()?;
    let player = rodio::Player::connect_new(device.mixer());
    player.append(SamplesBuffer::new(channels, sample_rate, samples));
    player.play();

    let started = Instant::now();
    let mut last_report = Duration::ZERO;

    while started.elapsed() < Duration::from_secs(3) {
        let wall = started.elapsed();
        if wall.saturating_sub(last_report) >= Duration::from_millis(100) {
            let audio = player.get_pos();
            let drift_ms = audio.as_secs_f64() * 1000.0 - wall.as_secs_f64() * 1000.0;
            println!(
                "audio_ms={:.1} wall_ms={:.1} drift_ms={drift_ms:.1} frame={:.0}",
                audio.as_secs_f64() * 1000.0,
                wall.as_secs_f64() * 1000.0,
                (audio.as_secs_f64() * FPS).floor()
            );
            last_report = wall;
        }
        sleep(Duration::from_millis(10));
    }

    player.stop();
    Ok(())
}

// SPDX-License-Identifier: GPL-3.0-only
//! SPK-4: validate the pre-decoded media manifest and the audio clock inputs.
//!
//! This binary does not play audio. It verifies the FFmpeg output, counts the
//! PNG sequence, reads the WAV with `hound`, and locates the beep onset. The
//! playback experiment lives in `src/bin/playback.rs` behind the `rodio`
//! feature; on this machine it is blocked by the missing ALSA development
//! package.

use std::error::Error;
use std::fs;

use hound::WavReader;

const FPS: f64 = 30.0;

fn main() -> Result<(), Box<dyn Error>> {
    let frames = fs::read_dir("frames")?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("frame_") && name.ends_with(".png"))
        })
        .count();

    let reader = WavReader::open("audio.wav")?;
    let spec = reader.spec();
    let samples: Vec<i16> = reader.into_samples::<i16>().collect::<Result<_, _>>()?;
    let channels = usize::from(spec.channels);
    let sample_rate = f64::from(spec.sample_rate);
    let duration_ms = samples.len() as f64 / f64::from(spec.channels) / sample_rate * 1000.0;

    let threshold = 1000_i32;
    let onset_sample = samples.chunks(channels).position(|frame| {
        frame
            .iter()
            .any(|sample| i32::from(*sample).abs() > threshold)
    });
    let onset_ms = onset_sample.map(|index| index as f64 / sample_rate * 1000.0);

    println!(
        "frames={frames} expected_frames={} fps={FPS}",
        (duration_ms / 1000.0 * FPS).round() as usize
    );
    println!(
        "wav sample_rate={} channels={} bits={} duration_ms={duration_ms:.1}",
        spec.sample_rate, spec.channels, spec.bits_per_sample
    );
    match onset_ms {
        Some(onset) => println!(
            "beep_onset_ms={onset:.1} beep_frame_index={:.1}",
            onset / 1000.0 * FPS
        ),
        None => println!("beep_onset_ms=not-found"),
    }

    Ok(())
}

// SPDX-License-Identifier: GPL-3.0-only

//! Pre-decoded media and audio for yet-rs.
//!
//! This crate is UI-free. [`PngSequence`] scans and decodes PNG frames;
//! [`WavAudio`] decodes PCM WAV with `hound`; [`RodioAudioPlayer`] plays it and
//! exposes the audio clock. The presenter owns pacing and presentation.

mod audio;
mod sequence;

pub use audio::{open_player, NullAudioPlayer, RodioAudioPlayer, WavAudio, DEFAULT_OUTPUT_LATENCY};
pub use sequence::{PngSequence, SequenceInfo};

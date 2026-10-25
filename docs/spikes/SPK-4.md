# SPK-4 — pre-decoded media and audio clock

**Status:** Media preparation go. Rodio API validated by Windows/macOS compile checks. Linux playback is blocked by the missing ALSA development package, which is a build-prerequisite decision.

**Question:** Can FFmpeg prepare a reproducible PNG sequence plus WAV, can Rust verify the manifest, and can `rodio` provide a usable audio clock for A/V pacing?

**Harness:** `spikes/spk4-media`.

## Media preparation

`prepare.sh` uses FFmpeg 6.1.1 to generate:

- 90 PNG frames, 320×240, 30 fps, 3 s;
- a 48 kHz, 2-channel, 16-bit WAV, 3.000 s;
- a 1 kHz beep at 1.0 s (`0.2 s` duration, delayed by 1 s).

Verification with `hound` 3.5.1:

```text
frames=90 expected_frames=90 fps=30
wav sample_rate=48000 channels=2 bits=16 duration_ms=3000.0
beep_onset_ms=1000.1 beep_frame_index=30.0
```

The manifest contract is consistent: frame 30 is the frame visible at the beep onset. This is the fixture for S8 A/V tests.

## Rodio 0.23 API findings

The UI analysis' suggested feature set is wrong for the current release:

- Rodio 0.23 has **no `hound` feature**. Its `wav` feature uses Symphonia (`MPL-2.0`).
- The current API is `DeviceSinkBuilder::open_default_sink()`, then `Player::connect_new(device.mixer())`.
- `SamplesBuffer` lives in `rodio::buffer`, not at the crate root.
- `Player` provides `append`, `play`, `pause`, `stop`, `clear`, `set_volume`, and `get_pos`.

The spike avoids Symphonia entirely: `hound` decodes the WAV into `f32` samples, and `SamplesBuffer::new(ChannelCount, SampleRate, Vec<f32>)` feeds the player. This keeps the decoder MIT-licensed and the dependency surface small.

Verified code shape:

```rust
let reader = hound::WavReader::open("audio.wav")?;
let spec = reader.spec();
let samples: Vec<f32> = reader
    .into_samples::<i16>()
    .map(|s| s.map(|v| f32::from(v) / 32768.0))
    .collect::<Result<_, _>>()?;

let device = rodio::DeviceSinkBuilder::open_default_sink()?;
let player = rodio::Player::connect_new(device.mixer());
player.append(rodio::buffer::SamplesBuffer::new(
    NonZeroU16::new(spec.channels).unwrap(),
    NonZeroU32::new(spec.sample_rate).unwrap(),
    samples,
));
player.play();
```

## Build matrix

| Target | Command | Result |
|---|---|---|
| Linux | `cargo check -p spk4-media --features rodio --bin playback` | **Blocked:** `alsa-sys` build script cannot find `alsa.pc`; `libasound2-dev` is missing. |
| Windows | `cargo check --target x86_64-pc-windows-msvc -p spk4-media --features rodio --bin playback` | **Compile pass.** Runtime untested. |
| macOS | `cargo check --target x86_64-apple-darwin -p spk4-media --features rodio --bin playback` | **Compile pass.** Runtime untested. |

The Linux failure is in the build script, before any Rust code is checked:

```text
The system library `alsa` required by crate `alsa-sys` was not found.
The file `alsa.pc` needs to be installed ...
```

The runtime ALSA library may be present on a desktop; the `-dev` package is required to build because `alsa-sys` needs `pkg-config` metadata.

## Decision

- **Adopt** `hound` + `rodio::buffer::SamplesBuffer` instead of the `wav`/Symphonia feature or the nonexistent `hound` feature.
- **Keep** `Player::get_pos()` as the audio clock, with the known caveat that it excludes hardware output-buffer latency.
- **Decide** how to treat ALSA on Linux:
  1. **Recommended:** document `libasound2-dev` (Debian/Ubuntu), `alsa-lib-devel` (Fedora), `alsa-lib` (Arch) as an OS-level audio build prerequisite, and add it to the Linux CI image. This is analogous to OS camera APIs and does not add a C/C++ compiler invocation to our code.
  2. Add an `audio` feature that can be disabled for Linux builds without ALSA; the session then runs silently and reports audio unavailable.
  3. Use an external `aplay`/`ffplay` process; rejected because it weakens the audio clock and adds a runtime dependency.
- **Do not** use the `wav`/Symphonia path unless the ALSA prerequisite is rejected and the MPL-2.0 license is accepted after review.

## Not measured here

- Real Linux playback, `get_pos()` drift, and underruns: blocked by the missing ALSA build metadata.
- Windows/macOS playback: compile-checked only.
- Observable A/V synchronization: requires the presenter integration in S8.

## Follow-ups

- Add ALSA dev to the Linux build/CI environment and rerun `playback`.
- Measure `get_pos()` vs wall clock and vs the PNG frame index on the generated fixture.
- Decide whether audio failure before a session is fatal by default (plan default: fail before the session; abort a trial on mid-trial failure).
- Recheck the `rodio` API and feature names before pinning; this spike verified 0.23.0.

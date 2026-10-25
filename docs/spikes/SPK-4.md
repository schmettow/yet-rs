# SPK-4 — pre-decoded media and audio clock

**Status:** Media preparation and Linux playback go. Rodio API validated by Windows/macOS compile checks and a Linux run. `get_pos()` runs ~128 ms behind wall clock on this system; S8 must compensate output latency.

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
| Linux | `cd spikes/spk4-media && cargo run --release -p spk4-media --features rodio --bin playback` | **Pass** after installing `libasound2-dev` (`alsa` 1.2.11). Playback and `get_pos()` measured. |
| Windows | `cargo check --target x86_64-pc-windows-msvc -p spk4-media --features rodio --bin playback` | **Compile pass.** Runtime untested. |
| macOS | `cargo check --target x86_64-apple-darwin -p spk4-media --features rodio --bin playback` | **Compile pass.** Runtime untested. |

The earlier Linux failure was in `alsa-sys`'s build script: `pkg-config` could not find `alsa.pc`. The `-dev` package is required to build, even when the runtime library is present. With `libasound2-dev` installed, the Linux build and playback succeed.

## Linux playback measurement

The playback binary polls `get_pos()` every 100 ms for 3 s. The stable offset was:

| Metric | Value |
|---|---:|
| Samples | 29 |
| Mean drift (audio − wall) | −127.9 ms |
| Median drift | −127.8 ms |
| Range | −150.9 … −100.5 ms |

`get_pos()` excludes the hardware output-buffer latency, exactly as the UI analysis warned. If video frames are selected directly from `get_pos()`, they lag the heard audio by roughly 0.13 s. This exceeds the proposed 50 ms A/V budget before compensation. S8 should measure a per-device offset and add it to `get_pos()` (or use a loopback/callback timestamp), then re-measure.

## Decision

- **Adopt** `hound` + `rodio::buffer::SamplesBuffer` instead of the `wav`/Symphonia feature or the nonexistent `hound` feature.
- **Accept** ALSA development metadata as a documented Linux build prerequisite; it was installed and the Linux playback run succeeded. Add it to the Linux CI image.
- **Keep** `Player::get_pos()` as the audio clock, but compensate the measured output latency. Do not assume `get_pos()` is the heard audio position.

## Not measured here

- Windows/macOS playback: compile-checked only.
- Observable A/V synchronization: requires the presenter integration in S8.
- Per-device output latency: measured on one device only; repeat on the lab machines.

## Follow-ups

- Add ALSA dev to the Linux CI image.
- In S8, measure the output-latency offset per device and apply it to the audio clock; re-measure drift against the PNG frame index.
- Decide whether audio failure before a session is fatal by default (plan default: fail before the session; abort a trial on mid-trial failure).
- Recheck the `rodio` API and feature names before pinning; this spike verified 0.23.0.

## S8 outcome (2026-10-25)

The integrated presenter was measured with the same 3 s, 30 fps clip and beep at
1 s. With the 128 ms compensation the console showed `audio drift +127.9 ms`; with
no offset it showed `media 2.10 s · audio drift +24.9 ms` at 2.1 s of experiment
time. `get_pos()` therefore tracks wall time once the stream is running, and the
spike's −128 ms was stream startup, not a steady-state buffer offset. The
default is now 0 (`audio.output_latency_ms`), and a per-device beep/flash
screen-and-sound recording is still required before claiming tight A/V sync.
Between 7 and 14 of 63 frames were skipped on the portrait presenter while it
painted; drops are counted and reported, and a prefetch worker is a follow-up.

# Sprint 1 spikes

Disposable harnesses for the four Sprint 1 risk spikes. This directory is a
separate Cargo workspace and is **excluded** from the root `yet-rs` workspace.
Nothing here is production code.

Reports are in `../docs/spikes/`:

- `SPK-1.md` — eframe presentation timing and HiDPI geometry
- `SPK-2.md` — desktop overlay matrix
- `SPK-3.md` — camera capture and quadrant parity
- `SPK-4.md` — pre-decoded media and audio clock

## Run

```sh
# SPK-1: X11/Wayland window; logs 180 frames, then exits
cd spk1-eframe && cargo run --release

# SPK-2: eframe overlay proxy (5 s)
cd spk2-egui-overlay && cargo run --release

# SPK-2: X11/XWayland mechanics proxy (5 s)
cd spk2-xwayland && cargo run --release

# SPK-2: layer-shell probe (needs a Wayland compositor)
cd spk2-layer-shell && cargo run

# SPK-3: camera capture (Linux, /dev/video0)
cd spk3-camera && cargo run --release
# MJPG instead of YUYV:
SPK3_FORMAT=MJPG cargo run --release
# Python parity fixture:
python3 parity.py
cargo run --release -- --fixture fixtures/quadrants.png

# SPK-4: media preparation and WAV verification
cd spk4-media && ./prepare.sh && cargo run --release
# Playback needs ALSA development metadata on Linux, or Windows/macOS:
cargo run --release --features rodio --bin playback
```

## Known environment limits

- Raw camera frames are ignored; only aggregate CSV and synthetic fixtures are kept.
- The current machine is X11 only; there is no Wayland compositor.
- Windows/macOS were compile-checked with `cargo check --target ...`, not run.
- Linux audio playback is blocked because `pkg-config` cannot find `alsa`.
- The macOS camera path is compile-blocked on Linux by `objc_exception`'s
  Objective-C build step; it needs Xcode Command Line Tools on a real Mac.

Do not merge this directory into the production workspace. Keep the reports,
fixtures and scripts; delete or archive the disposable Rust harnesses after the
spike is closed.

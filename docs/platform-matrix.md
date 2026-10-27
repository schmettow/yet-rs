# Platform and overlay matrix (OVL-8)

This is a **recording sheet**, not a claim of coverage. Fill a cell only after
running the check on that machine, with the date and environment in the row.
The Linux X11 entries were smoke-tested on the development workstation; the
Windows and macOS runtime cells are still open.

## Environment key

- **Linux X11** — KWin, two outputs, `XDG_SESSION_TYPE=x11` (deprecated path).
- **Linux Wayland** — KDE/Sway/wlroots with `zwlr_layer_shell_v1`; record the
  compositor and version.
- **Naked** — output-only Sway config from `tools/sway-yet.conf`, no panel,
  background service or notification daemon; `xwayland disable`.
- **Windows** — record the Windows build and GPU.
- **macOS** — record the macOS version and whether the display has a notch.

## Build and test coverage

| Target | Build check | Unit/integration tests | Runtime session |
|---|---|---|---|
| Linux x86_64 (this workstation) | ✅ `cargo check --workspace --all-targets --all-features` | ✅ 254 passed, 3 hardware tests ignored | ✅ console/presenter and camera paths exercised; overlay smoke below |
| Windows x86_64 | ✅ `cargo check --workspace --all-targets --all-features --target x86_64-pc-windows-msvc` | not executed here | ⬜ pending |
| macOS x86_64 | ⬜ blocked here: `objc_exception` needs an Apple SDK/Objective-C compiler | not executed here | ⬜ pending |
| Cortex-M (`thumbv6m`, `thumbv7em`) | ✅ `yet-core --no-default-features` | n/a | n/a |

## Overlay runtime matrix

Mark a row **pass** only when the marker follows the gaze point, clicks reach the
window below, no window loses focus, and the `--seconds 600` soak reports no
growing `outstanding` count.

| Environment | Backend | Marker | Click-through | No focus steal | HiDPI | Multi-monitor | Soak | Recorded by/date |
|---|---|---|---|---|---|---|---|---|
| Linux X11 KWin | `egui` best effort | ⬜ visual check pending | ⬜ | ⬜ | ⬜ | ⬜ | 30 s smoke: requested=868 painted=823 superseded=44 outstanding=1 errors=0 (27.4 painted/s) | dev workstation 2026-10-26 |
| Linux Wayland KDE | `layer-shell` | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | |
| Linux Wayland Sway | `layer-shell` | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | |
| Linux naked Sway | `layer-shell` | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | |
| Linux Wayland GNOME | unsupported | — | — | — | — | — | — | documented; `auto` disables with a notice |
| Windows | `egui` | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | |
| macOS | `egui` | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ | |

## Procedure

1. Build a release binary: `cargo build --release -p yet-zero`.
2. Start with the presenter monitor only. Confirm the marker is centered on the
   presenter output and not on the console:
   ```sh
   cargo run --release -p yet-zero -- overlay examples/demo-gui --seconds 10
   ```
   A click on a text editor below the marker must place the cursor; the editor
   must keep focus.
3. Check HiDPI at 100%, 150% and 200% scale. The marker radius is in physical
   pixels; it should not jump between outputs with different scales.
4. Check multi-monitor: move the marker, then change `display.presenter_monitor`
   and `[overlay] monitor`. v1 requires them to match and to be fullscreen.
5. Soak for 10 minutes, then 60 minutes before release:
   ```sh
   cargo run --release -p yet-zero -- overlay examples/demo-gui --seconds 3600
   ```
   Record `requested`, `painted`, `superseded`, `outstanding` and `errors`.
   `superseded` is expected when the overlay coalesces; `outstanding` should
   return to 0 or 1 after the run. A rising `outstanding`, a non-zero `errors`
   or a focus change is a failure.
6. For a naked compositor, use `tools/run-naked.sh` (see
   [naked-compositor.md](naked-compositor.md)) and repeat step 2 inside it.
7. Record the compositor/OS version, GPU, output layout and date in the table.

## Known limits

- GNOME Wayland has no supported overlay backend; see
  [overlay-decision.md](overlay-decision.md).
- X11 is best effort and deprecated. A failure there is expected to be recorded,
  not worked around.
- The layer-shell output order may differ from eframe's monitor index; use one
  output for data collection or verify the mapping after every compositor change.
- The 60-minute soak and the Windows/macOS runtime rows are release gates that
  cannot be completed by a compile check alone.

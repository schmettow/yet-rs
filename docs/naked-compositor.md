# Running yet-rs on a naked compositor

A naked Wayland compositor is the quietest way to run an experiment: no panel,
no desktop background service, no notification daemon and no window animations.
Yet-rs needs a layer-shell compositor for the gaze overlay, so **Sway** (wlroots)
is the reference. GNOME is not supported.

The checked-in profile is [`tools/sway-yet.conf`](../tools/sway-yet.conf); the
launcher [`tools/run-naked.sh`](../tools/run-naked.sh) fills in the experiment
path and output names.

## Requirements

- Linux with a working camera and, for the overlay, a layer-shell compositor.
- `sway` installed; `swaybg` only if you want an explicit background color.
- A release build of `yet-zero` (the launcher builds it if it is missing):
  ```sh
  cargo build --release -p yet-zero
  ```
- An experiment directory whose `config.toml` uses the monitor names/indices of
  this machine. The presenter monitor and `[overlay] monitor` must match.

## One-command start

From a TTY:

```sh
tools/run-naked.sh examples/demo-gui \
  --console-output DP-1 \
  --presenter-output DP-3 \
  --participant pilot01
```

The launcher renders a temporary Sway config, prints its path, and `exec`s
`sway -c <config>`. Use `swaymsg -t get_outputs` (inside a running session) or
your compositor's output list to find the names.

To inspect what would start without launching Sway:

```sh
tools/run-naked.sh examples/demo-gui \
  --console-output DP-1 --presenter-output DP-3 --render-only
```

`--binary`, `--participant` and `--config-out` override the defaults. The
launcher refuses a missing experiment directory and warns if `config.toml` is
absent.

## What the profile guarantees

```conf
set $console_output DP-1
set $presenter_output DP-3

default_border none
default_floating_border none
gaps inner 0
gaps outer 0
seat * hide_cursor 1000
focus_follows_mouse no
xwayland disable

for_window [app_id="yet-rs-console"] move to output $console_output
for_window [app_id="yet-rs-presenter"] fullscreen enable, move to output $presenter_output
```

- No bar, background service, notification daemon, clipboard manager or panel.
- No compositor animations or gaps.
- XWayland is disabled, so no foreign X11 client can appear above or beside the
  experiment. The overlay is a layer-shell surface, not an X11 client.
- The console and presenter app IDs are set by `yet-egui`; the overlay needs no
  window rule.
- `seat * hide_cursor 1000` hides the pointer after one second of inactivity.

The presenter should be the taller/portrait output if that is how the experiment
was authored. Edit the resolution/position lines in the template or keep the
defaults that match the lab workstation.

## Nested check

From an existing Wayland session, a nested Sway is useful for a quick overlay
check. It is not suitable for data collection:

```sh
WLR_BACKENDS=wayland WLR_RENDERER=pixman \
  tools/run-naked.sh examples/demo-gui --console-output HEADLESS-1 --presenter-output HEADLESS-2
```

From an X11 session, `WLR_BACKENDS=x11` may work if your wlroots build includes
the X11 backend. The launcher prints a note when `WAYLAND_DISPLAY` is already
set.

## Verify the overlay

Start a dedicated overlay run inside the session:

```sh
target/release/yet-zero overlay examples/demo-gui --seconds 10
```

The marker should follow a circle, clicks should reach the application below,
and no window should lose focus. Then soak:

```sh
target/release/yet-zero overlay examples/demo-gui --seconds 3600
```

The summary prints `requested`, `painted`, `superseded`, `outstanding` and
`errors`. `superseded` is expected (the newest gaze point wins); `outstanding`
should return to 0 or 1, and `errors` must be zero. Record the result in
[`docs/platform-matrix.md`](platform-matrix.md). The 60-minute run is a release
gate.

## Troubleshooting

| Symptom | Check |
|---|---|
| Sway does not start | Run from a TTY; a nested session needs the `WLR_BACKENDS` settings above |
| Console on the wrong output | Output name from `swaymsg -t get_outputs`; app ID `yet-rs-console` |
| Presenter not fullscreen | App ID `yet-rs-presenter`; the window rule must match |
| Overlay unavailable | `zwlr_layer_shell_v1`; use Sway/wlroots or KDE, not GNOME |
| Marker on the wrong output | Layer-shell output order; v1 requires `overlay.monitor == display.presenter_monitor` |
| No click-through | Use a layer-shell session; X11 is best effort and deprecated |

See [overlay-decision.md](overlay-decision.md) for the full backend decision and
the GNOME rationale.

## Exiting

The profile keeps a minimal keymap:

```conf
set $mod Mod4
bindsym $mod+Shift+e exit
bindsym $mod+Shift+r reload
```

Save your work in the console before exiting; an active session writes a partial
result file on abort or clean shutdown.

## What is deliberately absent

- No panel, tray, notification daemon, clipboard manager or background service.
- No compositor animations or gaps.
- No GNOME Shell extension and no XWayland overlay fallback.
- No runtime video decoder; stimuli are pre-decoded PNG sequences.

If you need a desktop with a panel for other work, run Yet in a normal KDE or
Sway session instead; the layer-shell backend works there too.

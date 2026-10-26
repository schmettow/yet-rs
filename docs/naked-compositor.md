# Running yet-rs on a naked compositor

A naked Wayland compositor is the quietest way to run an experiment: no panel,
no desktop background service, no notification daemon and no window animations.
Yet-rs needs a layer-shell compositor for the gaze overlay, so **Sway** (wlroots)
is the reference. GNOME is not supported.

## Requirements

- Linux with a working camera and, for the overlay, a layer-shell compositor.
- `sway` installed; `swaybg` only if you want an explicit background color.
- A release build of `yet-app`:
  ```sh
  cargo build --release -p yet-app
  ```
- An experiment directory whose `config.toml` uses the monitor names/indices of
  this machine. The presenter monitor and `[overlay] monitor` must match.

## Minimal Sway config

See [`tools/sway-yet.conf`](../tools/sway-yet.conf). Edit these values:

- `output DP-1 ...` and `output DP-3 ...`: your console and presenter outputs.
- `/path/to/yet-app` and `/path/to/experiment`: absolute paths.
- The presenter must be the taller/portrait output if that is how the experiment
  was authored.

The important rules are:

```conf
# No bar, no background service, no notifications.
default_border none
default_floating_border none
gaps inner 0
gaps outer 0
seat * hide_cursor 1000

for_window [app_id="yet-rs-console"] move to output DP-1
for_window [app_id="yet-rs-presenter"] fullscreen enable, move to output DP-3
exec /path/to/yet-app run /path/to/experiment --participant pilot01
```

The overlay is a layer-shell surface, so Sway composites it above the presenter
without a window rule. It uses an empty input region, so the mouse remains usable.

## Start

From a TTY (recommended):

```sh
sway -c /path/to/yet-rs/tools/sway-yet.conf
```

From an existing Wayland or X11 session, run Sway nested for a quick check:

```sh
WLR_BACKENDS=wayland WLR_RENDERER=pixman \
  sway -c /path/to/yet-rs/tools/sway-yet.conf
```

From an X11 session, `WLR_BACKENDS=x11` may work if your wlroots build includes
the X11 backend. Nested sessions are for validation, not for data collection.

## Exiting

The example config keeps a minimal keymap:

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

If you need a desktop with a panel for other work, run Yet in a normal KDE or
Sway session instead; the layer-shell backend works there too.

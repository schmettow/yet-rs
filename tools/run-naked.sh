#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-only
#
# Start a two-monitor naked Sway session for yet-rs.
#
#   tools/run-naked.sh <experiment-dir> [options]
#
# It renders tools/sway-yet.conf with this machine's paths and output names,
# then execs sway. Run it from a TTY; a nested Wayland session needs
# WLR_BACKENDS=wayland WLR_RENDERER=pixman (see docs/naked-compositor.md).
#
# Options:
#   --console-output NAME    Sway output for the console (default DP-1)
#   --presenter-output NAME  Sway output for the stimulus (default DP-3)
#   --participant ID         Participant id passed to yet-zero (default pilot01)
#   --binary PATH            yet-zero binary (default target/release/yet-zero)
#   --config-out PATH        Rendered Sway config (default $TMPDIR/yet-rs-sway.conf)
#   --render-only            Write the config and print its path; do not start sway
#   -h, --help               Show this help

set -eu

usage() {
    sed -n '4,19p' "$0" | sed 's/^# \{0,1\}//'
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)

experiment=
console_output=DP-1
presenter_output=DP-3
participant=pilot01
binary="$repo_root/target/release/yet-zero"
config_out="${TMPDIR:-/tmp}/yet-rs-sway.conf"
render_only=0
template="$script_dir/sway-yet.conf"

while [ "$#" -gt 0 ]; do
    case "$1" in
        --console-output)
            [ "$#" -ge 2 ] || { echo "error: --console-output needs a value" >&2; exit 2; }
            console_output=$2
            shift 2
            ;;
        --presenter-output)
            [ "$#" -ge 2 ] || { echo "error: --presenter-output needs a value" >&2; exit 2; }
            presenter_output=$2
            shift 2
            ;;
        --participant)
            [ "$#" -ge 2 ] || { echo "error: --participant needs a value" >&2; exit 2; }
            participant=$2
            shift 2
            ;;
        --binary)
            [ "$#" -ge 2 ] || { echo "error: --binary needs a value" >&2; exit 2; }
            binary=$2
            shift 2
            ;;
        --config-out)
            [ "$#" -ge 2 ] || { echo "error: --config-out needs a value" >&2; exit 2; }
            config_out=$2
            shift 2
            ;;
        --render-only)
            render_only=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        -*)
            echo "error: unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
        *)
            [ -z "$experiment" ] || { echo "error: only one experiment directory is accepted" >&2; exit 2; }
            experiment=$1
            shift
            ;;
    esac
done

[ -n "$experiment" ] || { echo "error: missing experiment directory" >&2; usage >&2; exit 2; }
[ -d "$experiment" ] || { echo "error: not a directory: $experiment" >&2; exit 1; }
[ -f "$template" ] || { echo "error: missing template: $template" >&2; exit 1; }

experiment=$(CDPATH= cd -- "$experiment" && pwd)

case "$binary" in
    /*) ;;
    *) binary="$repo_root/$binary" ;;
esac

if [ ! -f "$experiment/config.toml" ]; then
    echo "warning: $experiment/config.toml not found; yet-zero will reject the session" >&2
fi

if [ "$render_only" -eq 0 ] && [ ! -x "$binary" ]; then
    echo "building $binary ..."
    (cd "$repo_root" && cargo build --release -p yet-zero)
fi

awk -v c="$console_output" \
    -v p="$presenter_output" \
    -v b="$binary" \
    -v e="$experiment" \
    -v part="$participant" '
    /^set \$console_output / { print "set $console_output " c; next }
    /^set \$presenter_output / { print "set $presenter_output " p; next }
    /^exec / { print "exec \"" b "\" run \"" e "\" --participant \"" part "\""; next }
    { print }
' "$template" > "$config_out"

if [ "$render_only" -eq 1 ]; then
    printf '%s\n' "$config_out"
    exit 0
fi

command -v sway >/dev/null 2>&1 || {
    echo "error: sway is not installed; install it or use --render-only" >&2
    exit 1
}

if [ -n "${WAYLAND_DISPLAY:-}" ]; then
    echo "note: WAYLAND_DISPLAY is set; for a nested session use" >&2
    echo "      WLR_BACKENDS=wayland WLR_RENDERER=pixman tools/run-naked.sh ..." >&2
fi

echo "starting Sway on console=$console_output presenter=$presenter_output"
echo "config: $config_out"
exec sway -c "$config_out"

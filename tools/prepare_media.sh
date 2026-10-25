#!/usr/bin/env sh
# SPDX-License-Identifier: GPL-3.0-only
#
# Prepare a pre-decoded stimulus: PNG frames plus an optional WAV track.
#
#   tools/prepare_media.sh <video> <output-dir> [fps]
#
# The output directory gets `frames/frame_00001.png ...` and, when the input
# has an audio stream, `audio.wav` (48 kHz, stereo, 16-bit PCM).
# This is a setup step; the runtime never links FFmpeg.
set -eu

usage() {
    echo "usage: $0 <video> <output-dir> [fps]" >&2
    exit 2
}

[ "$#" -ge 2 ] || usage
input=$1
output=$2
fps=${3:-30}

if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "error: ffmpeg is required to prepare media but was not found on PATH" >&2
    exit 1
fi
if [ ! -f "$input" ]; then
    echo "error: input file not found: $input" >&2
    exit 1
fi
case "$fps" in
    ''|*[!0-9.]*|0|0.0)
        echo "error: fps must be a positive number, got '$fps'" >&2
        exit 2
        ;;
esac

mkdir -p "$output/frames"
rm -f "$output"/frames/frame_*.png "$output/audio.wav"

echo "extracting frames at $fps fps ..."
if ! ffmpeg -hide_banner -loglevel error -y \
    -i "$input" -vf "fps=$fps" "$output/frames/frame_%05d.png"; then
    echo "error: ffmpeg could not decode video from $input" >&2
    exit 1
fi

count=$(find "$output/frames" -name 'frame_*.png' | wc -l | tr -d ' ')
if [ "$count" -eq 0 ]; then
    echo "error: ffmpeg produced no frames; is $input a video file?" >&2
    exit 1
fi

audio_row=','
if ffprobe -v error -select_streams a:0 -show_entries stream=codec_type \
    -of csv=p=0 "$input" 2>/dev/null | grep -q audio; then
    echo "extracting audio ..."
    if ! ffmpeg -hide_banner -loglevel error -y -i "$input" -vn \
        -ac 2 -ar 48000 -c:a pcm_s16le "$output/audio.wav"; then
        echo "error: ffmpeg could not extract audio from $input" >&2
        exit 1
    fi
    audio_row=',audio.wav'
else
    echo "input has no audio stream; writing frames only"
fi

duration=$(ffprobe -v error -select_streams v:0 \
    -show_entries stream=duration -of default=nw=1:nk=1 "$input" 2>/dev/null || true)

echo "frames: $count"
if [ -n "${duration:-}" ]; then
    echo "source duration: ${duration}s"
fi
if [ -f "$output/audio.wav" ]; then
    echo "audio: $output/audio.wav"
fi
echo
echo "Add a row to Stimuli.csv (adjust id, duration_ms and loop):"
echo "  seq01,sequence,${output}/frames,$fps,3000,false${audio_row}"

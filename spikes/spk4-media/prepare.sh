#!/usr/bin/env sh
# SPK-4 media preparation: a 3 s, 30 fps PNG sequence and a WAV with a
# 1.0 s beep. Requires the FFmpeg CLI. Run from this directory.
set -eu

rm -rf frames
mkdir -p frames

ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i testsrc=size=320x240:rate=30:duration=3 \
    -frames:v 90 frames/frame_%05d.png

ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "sine=frequency=1000:duration=0.2" \
    -af "adelay=1000|1000,apad" \
    -t 3 -ar 48000 -ac 2 -c:a pcm_s16le audio.wav

printf 'frames=%s\n' "$(find frames -name 'frame_*.png' | wc -l)"
printf 'audio=%s\n' "$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 audio.wav)"

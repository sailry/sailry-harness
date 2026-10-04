#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "$0")"

# Synthetic media only. Existing fixtures are preserved; remove them explicitly to regenerate.
audio=(-hide_banner -loglevel error -n -f lavfi -i 'sine=frequency=440:sample_rate=8000:duration=0.1' -map_metadata -1)
encoder="${SAILRY_FFMPEG:-ffmpeg}"
"$encoder" "${audio[@]}" -c:a pcm_s16le audio.wav
"$encoder" "${audio[@]}" -c:a aac -b:a 16k -f mp4 -movflags +faststart audio.mp4
"$encoder" "${audio[@]}" -c:a libmp3lame -b:a 16k -id3v2_version 3 audio.mp3
video=(-hide_banner -loglevel error -n -f lavfi -i 'color=c=blue:s=16x16:r=4:d=0.5' -an -map_metadata -1)
"$encoder" "${video[@]}" -c:v libx264 -pix_fmt yuv420p -movflags +faststart video.mp4
"$encoder" "${video[@]}" -c:v libvpx-vp9 -b:v 10k video.webm

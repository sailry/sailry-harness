#!/usr/bin/env bash
set -euo pipefail

# Regenerate committed platform assets from the editable, approved SVG.
# Normal application builds consume these files without image conversion tools.
task_root="$(cd "$(dirname "$0")/.." && pwd)"
task_branding="$task_root/assets/branding"
command -v magick >/dev/null
command -v iconutil >/dev/null
task_icons="$(mktemp -d "${TMPDIR:-/tmp}/sailry-icons.XXXXXX")/Sailry.iconset"
mkdir "$task_icons"

magick -background none -density 192 "$task_branding/sailry-mark.svg" \
  -resize 1024x1024 -strip "PNG32:$task_branding/sailry.png"
magick "$task_branding/sailry.png" \
  -define icon:auto-resize=256,128,64,48,40,32,24,20,16 "$task_branding/sailry.ico"

# Only macOS gets a rounded white tile with transparent outer padding. An
# edge-to-edge white canvas is restyled as a gray plate by modern macOS.
# Keep the approved mark and the shared PNG/Windows ICO unchanged.
magick -size 1024x1024 xc:none -fill white \
  -draw 'roundrectangle 96,96 928,928 185,185' \
  \( "$task_branding/sailry.png" -resize 832x832 \) \
  -gravity center -composite "PNG32:${task_icons%/*}/macos.png"
for task_size in 16 32 128 256 512; do
  magick "${task_icons%/*}/macos.png" -resize "${task_size}x${task_size}" \
    -strip "PNG32:$task_icons/icon_${task_size}x${task_size}.png"
  task_double=$((task_size * 2))
  magick "${task_icons%/*}/macos.png" -resize "${task_double}x${task_double}" \
    -strip "PNG32:$task_icons/icon_${task_size}x${task_size}@2x.png"
done
iconutil -c icns "$task_icons" -o "$task_branding/Sailry.icns"
printf 'Updated application icons in %s\n' "$task_branding"

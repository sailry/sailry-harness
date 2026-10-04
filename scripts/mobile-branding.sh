#!/usr/bin/env bash
set -euo pipefail
task_root="$(cd "$(dirname "$0")/.." && pwd)"
task_branding="$task_root/apps/mobile/assets/branding"
mkdir -p "$task_branding"
# Reuse the Desktop vector mark. The packages generate each platform's assets.
magick -background none "$task_root/assets/branding/sailry-mark.svg" \
  -channel RGB -fill white -colorize 100 +channel -resize 600x600 \
  -gravity center -background none -extent 1024x1024 "$task_branding/mark.png"
magick "$task_branding/mark.png" -resize 288x288 "$task_branding/splash.png"
cd "$task_root/apps/mobile"
dart run flutter_launcher_icons
dart run flutter_native_splash:create

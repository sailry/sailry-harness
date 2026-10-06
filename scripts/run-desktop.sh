#!/usr/bin/env bash
set -euo pipefail

task_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$task_root"

task_native="$(rustc -vV | awk '/^host: / {print $2}')"
task_target="${CARGO_TARGET_DIR:-$task_root/target}"
python3 "$task_root/scripts/prepare-office-runtime.py" --target "$task_native" \
  --output "$task_target/debug/office-runtime"

if test "$(uname -s)" != Darwin; then
  exec cargo run --locked -p sailry-desktop -- "$@"
fi

# Ad hoc designated requirements bind to one build's cdhash, losing TCC grants
# after recompilation. Reuse a development identity instead (Apple TN3127).
task_identity="${SAILRY_CODESIGN_IDENTITY:-}"
if test -z "$task_identity"; then
  task_identity="$(security find-identity -v -p codesigning | awk '/"Apple Development: / {print $2; exit}')"
fi
if test -z "$task_identity"; then
  task_identity=-
fi
if test "$task_identity" = -; then
  printf '%s\n' 'Ad hoc signing: rebuilt apps may need permissions again. Set SAILRY_CODESIGN_IDENTITY to a signing certificate to retain app identity.' >&2
fi

# UNUserNotificationCenter requires a real application bundle, including in development.
cargo build --locked -p sailry-desktop
task_target="${CARGO_TARGET_DIR:-$task_root/target}"
task_build="$task_target/debug"
task_app="$task_build/Sailry.app"
mkdir -p "$task_app/Contents/MacOS" "$task_app/Contents/Resources"
if test -d "$task_app/Contents/Resources/office-runtime"; then
  rm -rf "$task_app/Contents/Resources/office-runtime"
fi
cp -R "$task_build/office-runtime" "$task_app/Contents/Resources/office-runtime"
if test -d "$task_app/Contents/Resources/hosts"; then
  rm -rf "$task_app/Contents/Resources/hosts"
fi
cp "$task_build/sailry-desktop" "$task_app/Contents/MacOS/sailry-desktop.next"
mv "$task_app/Contents/MacOS/sailry-desktop.next" "$task_app/Contents/MacOS/sailry-desktop"
cp "$task_root/apps/desktop/macos/Info.plist" "$task_app/Contents/Info.plist"
cp "$task_root/assets/branding/Sailry.icns" "$task_app/Contents/Resources/Sailry.icns"
cp -R "$task_root/apps/desktop/macos/zh_CN.lproj" "$task_app/Contents/Resources/"
codesign --force --timestamp=none --sign "$task_identity" "$task_app"
# LaunchServices gives Sailry its own TCC identity. Direct execution attributes
# microphone requests to the terminal's parent app, which may prohibit them.
# LaunchServices opens these paths independently, so /dev/stdout cannot relay
# an invoking shell's pipe. Use its terminal, or a log when no terminal exists.
if test -t 1 && task_console="$(tty)"; then
  task_log="$task_console"
else
  task_log="$(mktemp "${TMPDIR:-/tmp}/sailry-desktop.XXXXXX")"
  printf 'Application log: %s\n' "$task_log"
fi
exec /usr/bin/open -n -W --stdout "$task_log" --stderr "$task_log" "$task_app" --args "$@"

# Native microphone capture

`sailry_capture.c` is a small C shim over miniaudio 0.11.26, pinned by
`miniaudio.lock.json`. The Go application loads the shared library through
purego with cgo disabled (`internal/speech/capture`). miniaudio's structures
stay in C; the shim exports flat functions, and captured frames cross a
lock-free ring buffer, so the audio thread never enters Go.

`miniaudio.h` is not committed. `build.sh` reuses
`.runtime/native/src/miniaudio-0.11.26/miniaudio.h` when its SHA-256 equals
the lock, and otherwise downloads the pinned release archive and verifies the
archive and the header.

## Build

Artifacts are written to `.runtime/native/<goos>-<goarch>/`; none belong in
Git. The coordinating agent owns native builds.

| Target | Command | Output |
| --- | --- | --- |
| macOS, Linux (host architecture) | `sh native/capture/build.sh` | `libsailry-capture.dylib`, `libsailry-capture.so` |
| Windows (Git Bash, host architecture) | `sh native/capture/build.sh` | `sailry-capture.dll` |
| macOS, other architecture | `cc -O2 -fPIC -shared -arch x86_64 -mmacosx-version-min=11.0 -I<miniaudio> native/capture/sailry_capture.c -install_name @rpath/libsailry-capture.dylib -o libsailry-capture.dylib` | `libsailry-capture.dylib` |
| Windows (MSVC developer prompt) | `cl /O2 /LD /I<miniaudio> native\capture\sailry_capture.c /Fe:sailry-capture.dll` | `sailry-capture.dll` |

On Windows `build.sh` finds the Visual Studio C++ tools with `vswhere` and runs
`cl` through `vcvarsall.bat`; the last row is the same compilation by hand.

`<miniaudio>` is the directory holding the pinned `miniaudio.h`. Linux links
`-lpthread -lm -ldl`; miniaudio opens ALSA or PulseAudio at run time. macOS
loads CoreAudio and AudioToolbox at run time. Windows uses WASAPI.

## Interface

| Function | Meaning |
| --- | --- |
| `sailry_capture_version` | The miniaudio version compiled in; the binding requires `0.11.26` |
| `sailry_capture_devices` | One line per capture device: default flag, device, name |
| `sailry_capture_open` | Opens the default device (`NULL`) or a listed device as 32-bit float at its native rate and channels |
| `sailry_capture_sample_rate`, `sailry_capture_channels` | The opened format |
| `sailry_capture_start`, `sailry_capture_stop` | Run and pause the device |
| `sailry_capture_read` | Copies the interleaved frames captured so far |
| `sailry_capture_dropped` | Frames discarded because the reader fell more than two seconds behind |
| `sailry_capture_failed` | 1 once the device stopped by itself |
| `sailry_capture_close` | Releases the device |

A device is named by the hexadecimal form of its miniaudio device ID without
trailing zero bytes. It is stable for the backend that produced it and is the
value stored in `dictation.microphone`.

macOS microphone authorization is not part of the shim:
`internal/speech/capture` reads and requests it through AVFoundation. The
application bundle must carry `NSMicrophoneUsageDescription`.

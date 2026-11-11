#!/bin/sh
# Builds the microphone capture library for this machine into
# .runtime/native/<goos>-<goarch>/. The miniaudio header is the one pinned by
# miniaudio.lock.json: a cached copy is reused when its SHA-256 matches,
# otherwise the pinned release archive is downloaded and verified.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
source="$root/native/capture"
case "$(uname -s)" in
    Darwin) platform=darwin; name=libsailry-capture.dylib ;;
    Linux) platform=linux; name=libsailry-capture.so ;;
    # Git Bash on Windows; the library is compiled with MSVC.
    MINGW*|MSYS*) platform=windows; name=sailry-capture.dll ;;
    *) printf '%s\n' 'Unsupported native build platform; see native/capture/README.md' >&2; exit 1 ;;
esac
case "$(uname -m)" in
    arm64|aarch64) architecture=arm64 ;;
    x86_64|amd64) architecture=amd64 ;;
    *) printf '%s\n' 'Unsupported native build architecture' >&2; exit 1 ;;
esac

field() {
    sed -n "s/^ *\"$1\": *\"\\(.*\\)\",\\{0,1\\}$/\\1/p" "$source/miniaudio.lock.json"
}
digest() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}

version=$(field miniaudio_version)
archive=$(field archive)
archive_sha256=$(field archive_sha256)
header_sha256=$(field header_sha256)
include="$root/.runtime/native/src/miniaudio-$version"
header="$include/miniaudio.h"

if [ ! -f "$header" ] || [ "$(digest "$header")" != "$header_sha256" ]; then
    mkdir -p "$include"
    download="$root/.runtime/native/src/miniaudio-$version.tar.gz"
    curl --fail --location --silent --show-error --output "$download" "$archive"
    if [ "$(digest "$download")" != "$archive_sha256" ]; then
        printf '%s\n' 'miniaudio archive does not match miniaudio.lock.json' >&2
        exit 1
    fi
    tar -xzf "$download" -C "$include" --strip-components 1 "miniaudio-$version/miniaudio.h"
    if [ "$(digest "$header")" != "$header_sha256" ]; then
        printf '%s\n' 'miniaudio.h does not match miniaudio.lock.json' >&2
        exit 1
    fi
fi

output="$root/.runtime/native/$platform-$architecture"
mkdir -p "$output"
library="$output/$name"
if [ "$platform" = windows ]; then
    # cl runs inside the MSVC developer environment, which Git Bash does not
    # carry: a batch file enters it for this architecture and compiles there.
    case "$architecture" in
        arm64) tools=arm64 ;;
        *) tools=x64 ;;
    esac
    vswhere='/c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe'
    studio=$("$vswhere" -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | tr -d '\r')
    if [ -z "$studio" ]; then
        printf '%s\n' 'MSVC build tools are unavailable' >&2
        exit 1
    fi
    objects="$output/capture-objects"
    mkdir -p "$objects"
    batch="$objects/build.cmd"
    {
        printf '@echo off\r\n'
        printf 'call "%s\\VC\\Auxiliary\\Build\\vcvarsall.bat" %s >nul || exit /b 1\r\n' "$studio" "$tools"
        printf 'cl /nologo /O2 /LD /I"%s" "%s" /Fo"%s\\\\" /Fe:"%s" || exit /b 1\r\n' \
            "$(cygpath -w "$include")" "$(cygpath -w "$source/sailry_capture.c")" "$(cygpath -w "$objects")" "$(cygpath -w "$library")"
    } > "$batch"
    cmd.exe //d //c "$(cygpath -w "$batch")"
elif [ "$platform" = darwin ]; then
    case "$architecture" in
        arm64) target=arm64 ;;
        *) target=x86_64 ;;
    esac
    # miniaudio loads CoreAudio and AudioToolbox at run time.
    cc -O2 -fPIC -shared -arch "$target" -mmacosx-version-min=11.0 -I"$include" "$source/sailry_capture.c" \
        -install_name @rpath/libsailry-capture.dylib -o "$library"
else
    cc -O2 -fPIC -shared -I"$include" "$source/sailry_capture.c" -o "$library" -lpthread -lm -ldl
fi
printf '%s  %s\n' "$(digest "$library")" "$library"

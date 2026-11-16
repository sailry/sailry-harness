# Windows C++ runtime

The x64 native CI job runs `pwsh -File native/windows/crt.ps1` after building
iroh, Turso and capture. It uses `vswhere` to select the same installed Visual
Studio C++ toolchain as capture, reads its default redistributable version, and
copies the DLLs in `x64/Microsoft.VC*.CRT`. No runtime is downloaded separately.

The package needs `vcruntime140.dll` for the Rust libraries and also
`msvcp140.dll` and `vcruntime140_1.dll` for the pinned llama.cpp build. Shipping
the CRT directory together keeps its satellite DLLs from a single version.
Universal CRT API sets are supplied by the supported Windows operating system.
The original English Visual Studio license files and accompanying notices
are copied from `Licenses/1033` with a `windows-crt-` prefix. Missing required
DLLs or notices fail staging; no terms are synthesized.

`windows-crt.json` records the target, toolset version and filenames. The native
artifact's `SHA256SUMS` covers it, the DLLs and notices. `native/fetch` verifies
those checksums and puts the files beside the application executable. The seal
includes the complete list; Turso and iroh check it before loading their native
library. Python retains its own locked runtime files under `python/`.

For local Windows packaging, run the same PowerShell command first, then
`native/fetch` and the usual seal step. The Desktop installer and portable
archive both include the staged runtime. Reuse the native artifact for other
platform packaging; a cross-build does not establish Windows runtime behavior.

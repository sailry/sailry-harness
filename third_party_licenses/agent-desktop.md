# agent-desktop

Sailry uses unmodified official agent-desktop **v1.2.0** release executables from
https://github.com/vercel-labs/agent-desktop/releases/tag/v1.2.0, built by the
upstream release workflow from commit `2b0a7334fc111730454b726e6800aa41cdafc093`.
The locked archive and member identities are in
`scripts/package/agent-desktop.lock.json`, copied into distribution notices.
agent-desktop is distributed under the retained Apache-2.0 license in
[agent-desktop-Apache-2.0.txt](agent-desktop-Apache-2.0.txt); the upstream
repository has no NOTICE file. Its statically linked Rust dependencies are listed
in the upstream `Cargo.lock` at that commit.

On macOS, Sailry places `agent-desktop` and `agent-desktop-macos-helper` in its
own `Sailry Computer Use.app` bundle and replaces the upstream ad hoc signatures
with its own. Windows and Linux executables are staged unchanged beside Desktop
or Host. The Linux executables require glibc 2.35 or newer.

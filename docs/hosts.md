# Remote hosts

A Host runs tasks on another machine. Disconnecting Desktop does not stop tasks
already accepted by that Host. Its projects, models and credentials stay there.

## Prevent sleep

**Settings → General → Prevent sleep** controls the selected execution Node and
is enabled by default. While enabled, that device stays awake even when no task
is running. Turning it off or shutting down the Node releases the request.
Disconnecting a controller does not change the Node's setting.

This prevents idle system sleep on macOS and Windows, while allowing the display
to turn off. It does not prevent locking, lid-close sleep, or explicit sleep.
Windows Modern Standby can limit requests on battery power. Linux sleep prevention
is not implemented. If the system rejects a request, Sailry continues running and
reports the failure once for that Node process.

## Before connecting

- Use macOS 13+ on Apple silicon or Intel, or Linux with glibc 2.34+ on ARM64 or
  x64. The installer does not support Windows.
- The matching Sailry release must include Host packages for the target machine.
  **Add host** installs the Host version expected by your Desktop build; it
  does not choose the latest stable or preview release or fall back to another version.
- Have a reachable SSH server, its username, and a password or private key.
  The remote account needs permission to install and run the service.
- The target needs `bash`, `curl` and `tar`, plus HTTPS access to GitHub downloads.
  Linux needs systemd. Non-root installs require a working user service manager
  and permission to enable lingering. macOS needs a signed-in user's launchd GUI session.

Linux also needs the libX11 and libXi runtime libraries, including on headless
servers. Before installing Host, install `libX11` and `libXi` on RHEL 9 compatible
systems, or `libx11-6` and `libxi6` on Debian/Ubuntu.

Local decisions also need OpenMP: `libgomp` on RHEL-compatible systems or
`libgomp1` on Debian/Ubuntu. Decision inference requires glibc 2.34+ on x64 or
2.38+ on ARM64; Host, OCR and embedding retain their 2.34 minimum. Restart Host
after installing native dependencies so its cached availability is checked again.

## Install and pair

1. Choose **Add host** and enter the SSH host, port, username and authentication.
   Private keys can be pasted or loaded from a key file; supply its passphrase if needed.
2. Choose **Install and connect**. Verify the SSH host-key fingerprint before
   trusting a new host, especially if a previously saved key has changed.
3. Wait for installation and pairing, then select the added host in the sidebar.
   [Configure its model provider](models.md) and [add a project](projects.md) there.

## If installation fails

If the matching version's Host archive or installer is missing from the release,
installation fails. Check the [published packages](https://github.com/sailry/sailry-harness/releases)
for that version and architecture; changing the Desktop update channel does not
make a different Host version eligible.

For authentication or host-key errors, verify the SSH account and fingerprint.
For service errors, check systemd or launchd on the target. An unconfirmed result
can mean installation or startup happened but pairing did not finish; inspect the
remote service before starting another installation.

The fresh installer refuses an existing Host installation or data profile.
Do not delete these to retry. Existing service controls and profile-preserving
updates are described in [Host automation](../CONTRIBUTING.md#host-automation).

## Telemetry and privacy

**Settings → General → Diagnostics and privacy** has separate switches for error
reports and anonymous usage. Both are enabled by default. Changes apply
immediately and are saved with that profile. Resetting the anonymous ID creates
a new random installation ID without changing either switch or conversation data.

These Desktop controls always use the Desktop's local profile, even while a
remote Host is selected. Each Host process uses its own profile and installation
ID. A remote connection does not copy the Desktop's reporting choices to Host.
Host settings use the same revision-checked Client commands for reading, saving
and resetting telemetry preferences.

Reports contain the application version and source revision, release channel,
Desktop or Host type, OS and CPU architecture, and random installation ID.
Error reports contain error codes and sanitized application stack frames.
Anonymous usage contains aggregated launch, turn, tool, model and search counts,
outcomes, timings and token totals. Built-in model names and tools from bundled
or official plugins may be included; custom models, third-party plugin tools and
MCP tools use generic labels. Error reports and diagnostics include plugin names
and versions only when the exact shipped package identity is verified.
Usage is aggregated in memory and sent about every 15 minutes, with a bounded
attempt during shutdown.

Conversations, prompts, answers, file contents, credentials, local filesystem
paths, custom project or provider names, and MCP server names are excluded.
Turning a reporting switch off discards its pending data. Reporting failures do
not block tasks.

**Send diagnostics** explicitly sends a sanitized excerpt from the latest 500
local log lines, with the same field restrictions. It remains available when
automatic error reports are off; a toast reports whether delivery succeeded.

To disable all reporting for a process, set `SAILRY_TELEMETRY=off` or
`DO_NOT_TRACK=1` before starting Desktop or Host. Host also accepts
`sailry-host --telemetry off`; `--telemetry on` permits reporting only when the
environment and saved preferences allow it. These process overrides leave saved
preferences unchanged. Environment opt-outs also disable manual diagnostics.
For an installed service, set the environment or arguments in its service
configuration and restart it.

### Source builds and debug symbols

`SAILRY_SENTRY_DSN` configures error delivery to Sentry, and
`SAILRY_TELEMETRY_KEY` configures anonymous usage delivery to Telemetry.sh.
The same names can be provided at compile time or overridden in the runtime
environment. An explicitly empty runtime value disables that destination.
Builds without a configured destination do not send that category.
Normal delivery runs in the background. Shutdown waits at most two seconds;
a panic hook also waits at most two seconds to deliver its crash report.

For local builds, the Desktop and Host recipes load those two values from the
ignored root `.env.telemetry` file through `scripts/telemetry-env.py`. Explicit
environment values take precedence, including empty values. The loader does not
evaluate shell substitutions. Keep this file private. GitHub release builds
receive the same names through repository Secrets; actual values do not belong
in source, build metadata or documentation.

Release build steps set `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only` and
`CARGO_PROFILE_RELEASE_STRIP=none` so matching native symbols retain source-line
information. The ordinary release profile removes that information; collecting
symbols afterward cannot recover it. These overrides are confined to release
build steps and do not change development profiles or signing.

Release workflows preserve dSYM bundles and ELF debug files as separate
`debug-symbols-*` artifacts for 14 days, even when upload credentials are absent.
Local macOS, Linux Host and Windows packaging collect existing dSYM, ELF debug or
PDB files when available, without rebuilding. Symbols stay under
`target/debug-symbols/<component>/<target>` and outside application archives.

Application packages retain function symbol tables so stack frames can be
resolved locally before privacy filtering. Reports also retain instruction
addresses and debug-image identifiers so Sentry can match frames to uploaded
symbols for server-side symbolication. Absolute image paths are removed before
reporting. Linux Host's packaged copy has its debug sections removed, and macOS
line information lives in a separate dSYM. Without matching uploaded symbols,
production reports can contain function names without source filenames or line
numbers. Separate symbol artifacts remain available for offline debugging.

Uploading symbols requires a Sentry API token in the `SENTRY_AUTH_TOKEN` Secret,
the organization slug in `SENTRY_ORG`, and the project slug in `SENTRY_PROJECT`.
Slugs can be set in repository Variables or Secrets. A DSN does not supply these
upload credentials. The helper downloads the pinned, SHA-256-verified Sentry CLI
from its committed lockfile and uploads dSYM, ELF debug or PDB files to hosted
Sentry using the CLI's default `https://sentry.io/` endpoint. Source-bundle uploads
are disabled. Without all three values it preserves the artifact and skips
upload. Configured upload failures fail the workflow after the symbol artifact
has been saved.

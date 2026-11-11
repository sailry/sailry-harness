# Remote hosts

A Host runs tasks on another machine. Disconnecting Desktop does not stop tasks
already accepted by that Host. Its projects, models and credentials stay there.

## Prevent sleep

**Settings → General → Prevent sleep** controls the selected execution Node and
is enabled by default. While enabled, that device stays awake for as long as
work runs on it: a task, a command, an installation, knowledge indexing, a code
index query or a backup. An idle device may sleep, and an open terminal or a
schedule that has not fired yet does not keep it awake; a scheduled task or
reminder that came due during sleep runs shortly after the device wakes.
Turning the setting off or shutting down the Node releases the request.
Disconnecting a controller does not change the Node's setting.

This prevents idle system sleep on macOS and Windows, while allowing the display
to turn off. It does not prevent locking, lid-close sleep, or explicit sleep.
Windows Modern Standby can limit requests on battery power. Linux sleep prevention
is not implemented. If the system rejects a request, Sailry continues running,
reports the failure once for that Node process and asks again with the next work.

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
updates are described in [Host automation](../../CONTRIBUTING.md#host-automation).

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

### Source builds

`SAILRY_SENTRY_DSN` configures error delivery to Sentry, and
`SAILRY_TELEMETRY_KEY` configures anonymous usage delivery to Telemetry.sh.
A non-empty runtime environment value takes precedence over the value compiled
into the build. Builds without a configured destination do not send that
category.

Release builds compile the destinations in through the Go linker variables
`telemetry.BuildDSN`, `telemetry.BuildKey` and `telemetry.BuildRevision`, as
described in [Release packages](../../CONTRIBUTING.md#release-packages). GitHub
release builds receive the values through repository Secrets with the same
names; actual values do not belong in source, build metadata or documentation.

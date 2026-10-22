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

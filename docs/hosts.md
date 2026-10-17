# Remote hosts

A Host runs tasks on another machine. Disconnecting Desktop does not stop tasks
already accepted by that Host. Its projects, models and credentials stay there.

## Before connecting

- Use macOS 13+ on Apple silicon or Intel, or Linux with glibc 2.28+ on ARM64 or
  x64. The installer does not support Windows.
- The matching Sailry release must include Host packages for the target machine.
  **Add host** installs the Host version expected by your Desktop build; it
  does not choose the latest stable or preview release or fall back to another version.
- Have a reachable SSH server, its username, and a password or private key.
  The remote account needs permission to install and run the service.
- The target needs `bash`, `curl` and `tar`, plus HTTPS access to GitHub downloads.
  Linux needs systemd. Non-root installs require a working user service manager
  and permission to enable lingering. macOS needs a signed-in user's launchd GUI session.

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

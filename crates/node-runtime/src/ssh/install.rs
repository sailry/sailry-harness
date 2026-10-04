//! Deployment follows sailry-code 67ae9fa0 host-bootstrap, using the shared Node SSH owner.
use super::*;
use sailry_link::LinkHandle;
use sailry_protocol::{NodeId, ssh::InstallProgress};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tokio::{io::AsyncReadExt, sync::watch};

pub(super) async fn execute(
    session: &client::Handle<Verifier>,
    link: &LinkHandle,
    progress: &watch::Sender<InstallProgress>,
) -> Result<Outcome, Fault> {
    progress.send_replace(InstallProgress::Detecting);
    let platform = command(session, "uname -s; uname -m").await?;
    let target = match platform.split_whitespace().collect::<Vec<_>>().as_slice() {
        ["Linux", "x86_64"] => "x86_64-unknown-linux-gnu",
        ["Linux", "aarch64" | "arm64"] => "aarch64-unknown-linux-gnu",
        ["Darwin", "arm64"] => "aarch64-apple-darwin",
        ["Darwin", "x86_64"] => "x86_64-apple-darwin",
        _ => {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "Host installation supports Linux and macOS on x86_64 or ARM64",
            ));
        }
    };
    let artifact = artifact(target)?;
    let runtime = artifact.with_file_name("office-runtime.tar.gz");
    let size = |path: &std::path::Path| -> Result<u64, Fault> {
        let size = std::fs::metadata(path)
            .map_err(|_| unavailable("Bundled host or Office runtime is missing"))?
            .len();
        if size == 0 || size > 1024 * 1024 * 1024 {
            return Err(unavailable("Invalid host artifact size"));
        }
        Ok(size)
    };
    let binary_size = size(&artifact)?;
    let total = binary_size + size(&runtime)?;
    // Never replace an existing service or profile through the Add Host workflow.
    command(session, r#"set -eu
if test -e "$HOME/.local/lib/sailry/sailry-host" || test -e "$HOME/.sailry-host" || test -e "$HOME/.config/systemd/user/sailry-host.service" || test -e "$HOME/Library/LaunchAgents/ai.sailry.host.plist"; then echo 'Sailry Host is already installed; pair with the existing host' >&2; exit 1; fi
if test "$(uname -s)" = Linux; then command -v systemctl >/dev/null; if test "$(id -u)" = 0; then test ! -e /etc/systemd/system/sailry-host.service; fi; else launchctl print "gui/$(id -u)" >/dev/null; fi
umask 077
mkdir -p "$HOME/.local/lib/sailry"
"#).await?;
    upload(
        session,
        artifact,
        "sailry-host.new",
        target,
        progress,
        0,
        total,
    )
    .await?;
    upload(
        session,
        runtime,
        "office-runtime.tar.gz",
        target,
        progress,
        binary_size,
        total,
    )
    .await?;
    progress.send_replace(InstallProgress::Installing);
    command(
        session,
        r#"set -eu
cd "$HOME/.local/lib/sailry"
test ! -e office-runtime
tar -xzf office-runtime.tar.gz
rm office-runtime.tar.gz
"#,
    )
    .await?;
    command(session, "set -eu; chmod 700 \"$HOME/.local/lib/sailry/sailry-host.new\"; mv \"$HOME/.local/lib/sailry/sailry-host.new\" \"$HOME/.local/lib/sailry/sailry-host\"").await?;
    command(session, include_str!("install/service.sh")).await?;
    progress.send_replace(InstallProgress::Pairing);
    for _ in 0..90 {
        if let Outcome::Completed {
            exit_code: 0,
            stdout,
            truncated: false,
            ..
        } = run(
            session,
            "cat \"$HOME/.sailry-host/bootstrap.ticket\" 2>/dev/null",
        )
        .await?
            && !stdout.is_empty()
        {
            let address = link.pair(stdout.trim()).await?;
            let execution = link.inspect(&address).await?.execution;
            if !execution {
                return Err(unavailable("Installed endpoint does not provide a Node"));
            }
            return Ok(Outcome::HostInstalled {
                node: NodeId(*address.id.as_bytes()),
            });
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err(unknown(
        "Host service started but pairing is unavailable; inspect the host service",
    ))
}

async fn upload(
    session: &client::Handle<Verifier>,
    artifact: PathBuf,
    name: &'static str,
    target: &str,
    progress: &watch::Sender<InstallProgress>,
    offset: u64,
    total: u64,
) -> Result<(), Fault> {
    let file = tokio::fs::File::open(&artifact)
        .await
        .map_err(|_| unavailable("Host artifact cannot be opened"))?;
    let size = file
        .metadata()
        .await
        .map_err(|_| unavailable("Host artifact metadata is unavailable"))?
        .len();
    if size == 0 || size > 1024 * 1024 * 1024 {
        return Err(unavailable("Invalid host artifact size"));
    }
    let checksum = tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(artifact)?;
        let mut hash = Sha256::new();
        std::io::copy(&mut file, &mut hash)?;
        Ok::<_, std::io::Error>(format!("{:x}", hash.finalize()))
    })
    .await
    .map_err(|_| unavailable("Host artifact verification interrupted"))?
    .map_err(|_| unavailable("Host artifact cannot be verified"))?;
    progress.send_replace(InstallProgress::Uploading {
        sent: offset,
        total,
    });
    let mut channel = session
        .channel_open_session()
        .await
        .map_err(|_| unavailable("SSH upload channel could not be opened"))?;
    channel
        .exec(
            true,
            format!("umask 077; cat > \"$HOME/.local/lib/sailry/{name}\""),
        )
        .await
        .map_err(|_| unknown("Host upload could not start"))?;
    let mut file = file;
    let mut buffer = vec![0; 64 * 1024];
    let mut sent = offset;
    loop {
        let count = file
            .read(&mut buffer)
            .await
            .map_err(|_| unavailable("Host artifact read failed"))?;
        if count == 0 {
            break;
        }
        channel
            .data(&buffer[..count])
            .await
            .map_err(|_| unknown("Host upload was interrupted"))?;
        sent += count as u64;
        progress.send_replace(InstallProgress::Uploading { sent, total });
    }
    channel
        .eof()
        .await
        .map_err(|_| unknown("Host upload was interrupted"))?;
    let mut uploaded = false;
    while let Some(message) = channel.wait().await {
        if let ChannelMsg::ExitStatus { exit_status } = message {
            uploaded = exit_status == 0;
        }
    }
    if !uploaded {
        return Err(unknown("Host upload failed"));
    }
    let checksum_command = if target.contains("linux") {
        "sha256sum"
    } else {
        "shasum -a 256"
    };
    let remote = command(
        session,
        &format!("{checksum_command} \"$HOME/.local/lib/sailry/{name}\""),
    )
    .await?;
    if remote.split_whitespace().next() != Some(&checksum) {
        return Err(unavailable("Uploaded host checksum does not match"));
    }
    Ok(())
}

async fn command(session: &client::Handle<Verifier>, command: &str) -> Result<String, Fault> {
    match run(session, command).await? {
        Outcome::Completed {
            exit_code: 0,
            stdout,
            truncated: false,
            ..
        } => Ok(stdout),
        Outcome::Completed { stderr, .. } => Err(Fault::new(
            ErrorCode::Unavailable,
            format!("Host installation command failed: {}", stderr.trim()),
        )),
        _ => Err(unknown("Host installation command outcome is unavailable")),
    }
}

fn artifact(target: &str) -> Result<PathBuf, Fault> {
    let executable =
        std::env::current_exe().map_err(|_| unavailable("Application path unavailable"))?;
    let mut roots = Vec::new();
    if cfg!(debug_assertions) {
        if let Some(path) = std::env::var_os("SAILRY_HOST_ARTIFACT_DIR") {
            roots.push(PathBuf::from(path));
        }
        roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/host-artifacts"));
    }
    if let Some(parent) = executable.parent() {
        roots.push(parent.join("../Resources/hosts"));
        roots.push(parent.join("hosts"));
        roots.push(parent.join("host-artifacts"));
    }
    roots
        .into_iter()
        .map(|root| root.join(target).join("sailry-host"))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            Fault::new(
                ErrorCode::NotFound,
                format!("Bundled host artifact is missing for {target}"),
            )
        })
}

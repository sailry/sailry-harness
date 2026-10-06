//! Official release deployment uses the shared Node SSH owner and preserves existing profiles.
//! The Add Host entry follows sailry-code 67ae9fa0 host-bootstrap.
use super::*;
use sailry_link::LinkHandle;
use sailry_protocol::{NodeId, ssh::InstallProgress};
use tokio::sync::watch;

pub(super) async fn execute(
    session: &client::Handle<Verifier>,
    link: &LinkHandle,
    progress: &watch::Sender<InstallProgress>,
) -> Result<Outcome, Fault> {
    progress.send_replace(InstallProgress::Detecting);
    let platform = command(session, "uname -s; uname -m").await?;
    if !matches!(
        platform.split_whitespace().collect::<Vec<_>>().as_slice(),
        ["Linux", "x86_64" | "aarch64" | "arm64"] | ["Darwin", "arm64" | "x86_64"]
    ) {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "Host installation supports Linux and macOS on amd64 or arm64",
        ));
    }
    // Deployment is pinned to this application version, not a mutable latest release.
    let version = env!("SAILRY_HOST_VERSION");
    let script = format!(
        "install_version='{version}'\n{}",
        include_str!("install/release.sh")
    );
    progress.send_replace(InstallProgress::Installing);
    command(session, &script).await?;
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
            if !link.inspect(&address).await?.execution {
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

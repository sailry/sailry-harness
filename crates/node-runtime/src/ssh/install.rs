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
    expected: Option<NodeId>,
) -> Result<Outcome, Fault> {
    progress.send_replace(InstallProgress::Detecting);
    let platform = command(session, "uname -s; uname -m").await?;
    if !matches!(
        platform.split_whitespace().collect::<Vec<_>>().as_slice(),
        ["Linux", "x86_64" | "aarch64" | "arm64"] | ["Darwin", "arm64" | "x86_64"]
    ) {
        return Err(Fault::invalid(
            "Host installation supports Linux and macOS on amd64 or arm64",
        ));
    }
    // Deployment is pinned to this application version, not a mutable latest release.
    let version = env!("SAILRY_HOST_VERSION");
    if let Some(node) = expected {
        let identity = command(
            session,
            concat!(
                "\"$HOME/.local/lib/sailry/python/bin/python3.12\" -I - <<'SAILRY_IDENTITY'\n",
                include_str!("install/identity.py"),
                "\nSAILRY_IDENTITY"
            ),
        )
        .await?;
        let identity: Identity = serde_json::from_str(&identity)
            .map_err(|_| unavailable("Managed Host identity could not be verified"))?;
        validate_update(&identity, node, version)?;
    }
    let script = format!(
        "install_version='{version}'\ninstall_update={}\n{}",
        if expected.is_some() { "1" } else { "0" },
        include_str!("install/release.sh")
    );
    progress.send_replace(InstallProgress::Installing);
    command(session, &script).await?;
    progress.send_replace(InstallProgress::Pairing);
    if let Some(node) = expected {
        link.disconnect(node).await;
        for _ in 0..90 {
            for address in link.peers().await? {
                if NodeId(*address.id.as_bytes()) == node
                    && let Ok(info) = link.inspect(&address).await
                    && info.execution
                    && info.compatibility().is_ok()
                {
                    return Ok(Outcome::HostInstalled { node });
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        return Err(unknown(
            "Host update was started but reconnection could not be confirmed",
        ));
    }
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

#[derive(serde::Deserialize)]
struct Identity {
    node: NodeId,
    version: String,
}

fn validate_update(identity: &Identity, node: NodeId, target: &str) -> Result<(), Fault> {
    if identity.node != node {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "SSH endpoint belongs to another Host",
        ));
    }
    let current = identity
        .version
        .strip_prefix("Sailry Host ")
        .and_then(|value| semver::Version::parse(value).ok())
        .ok_or_else(|| unavailable("Installed Host version could not be verified"))?;
    let target =
        semver::Version::parse(target).map_err(|_| unavailable("Invalid target Host version"))?;
    if current.cmp_precedence(&target).is_gt() {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "Host is newer; update the controller instead",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confines_updates_to_existing_host() {
        let node = NodeId([1; 32]);
        let mut identity = Identity {
            node,
            version: "Sailry Host 0.1.0-alpha.1".into(),
        };
        assert!(validate_update(&identity, node, "0.1.0-alpha.2").is_ok());
        assert_eq!(
            validate_update(&identity, NodeId([2; 32]), "0.1.0-alpha.2")
                .unwrap_err()
                .code,
            ErrorCode::WrongTarget
        );
        assert_eq!(
            validate_update(&identity, node, "0.1.0-alpha.0")
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        identity.version = "invalid".into();
        assert!(validate_update(&identity, node, "0.1.0-alpha.2").is_err());
    }
}

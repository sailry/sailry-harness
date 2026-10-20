//! Official release deployment uses the shared Node SSH owner and preserves existing profiles.
//! The Add Host entry follows sailry-code 67ae9fa0 host-bootstrap.
use super::*;
use sailry_link::LinkHandle;
use sailry_protocol::{NodeId, RequestId, network::Selection, ssh::InstallProgress};
use tokio::sync::watch;

pub(super) async fn execute(
    session: &client::Handle<Verifier>,
    link: &LinkHandle,
    progress: &watch::Sender<InstallProgress>,
    expected: Option<NodeId>,
    network: Option<(RequestId, Selection)>,
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
                "install_root=\"$HOME/.local/lib/sailry\"\n",
                "if test \"$(uname -s)\" = Linux && test \"$(id -u)\" = 0; then\n",
                "  if test -e /usr/local/libexec/sailry; then install_root=/usr/local/libexec/sailry\n",
                "  elif ! test -L /usr/local/bin/sailry || test \"$(readlink /usr/local/bin/sailry)\" != \"$install_root/sailry\"; then\n",
                "    echo 'Managed Host program files are unavailable' >&2; exit 1\n",
                "  fi\n",
                "fi\n",
                "\"$install_root/python/bin/python3.12\" -I - \"$install_root\" <<'SAILRY_IDENTITY'\n",
                include_str!("install/identity.py"),
                "\nSAILRY_IDENTITY"
            ),
        )
        .await?;
        let identity: Identity = serde_json::from_str(&identity)
            .map_err(|_| unavailable("Managed Host identity could not be verified"))?;
        validate_update(&identity, node, version)?;
    }
    let script = script(
        version,
        expected.is_some(),
        network.as_ref().map(|(_, selection)| selection),
    )?;
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
        .await
        .map_err(|_| unknown("Host installed but its bootstrap ticket could not be read"))?
            && !stdout.is_empty()
        {
            let address = link
                .pair(stdout.trim())
                .await
                .map_err(|_| unknown("Host installed but pairing could not be confirmed"))?;
            if !link
                .inspect(&address)
                .await
                .map_err(|_| unknown("Host installed but its endpoint could not be confirmed"))?
                .execution
            {
                return Err(unknown("Installed endpoint does not provide a Node"));
            }
            if let Some((request, selection)) = &network
                && configure_network(link, address.clone(), *request, selection)
                    .await
                    .is_err()
            {
                return Ok(Outcome::HostNetworkUnconfirmed {
                    node: NodeId(*address.id.as_bytes()),
                    selection: selection.clone(),
                });
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

fn script(version: &str, update: bool, network: Option<&Selection>) -> Result<String, Fault> {
    let mut arguments = Vec::new();
    match network {
        Some(Selection::Iroh) => arguments.push("'--internet'".into()),
        Some(Selection::Custom { relays }) => {
            for relay in relays {
                if relay.contains('\0') {
                    return Err(Fault::invalid("relay URL contains a null byte"));
                }
                arguments.push("'--relay'".into());
                arguments.push(format!("'{}'", relay.replace('\'', "'\"'\"'")));
            }
        }
        None => {}
    }
    Ok(format!(
        "install_version='{version}'\ninstall_update={}\nset -- {}\n{}",
        if update { "1" } else { "0" },
        arguments.join(" "),
        include_str!("install/release.sh")
    ))
}

async fn configure_network(
    link: &LinkHandle,
    address: sailry_link::EndpointAddr,
    id: RequestId,
    selected: &Selection,
) -> Result<(), Fault> {
    use sailry_protocol::{Command, Output, Request};
    let target = NodeId(*address.id.as_bytes());
    let remote = link.remote(address);
    let read = || Request::new(target, Command::ReadNetworkSettings);
    let admission = remote.dispatch(read()).await?;
    let Output::NetworkSettings(settings) = admission
        .completion
        .await
        .map_err(|_| unknown("network settings response was lost"))??
    else {
        return Err(Fault::invalid("network settings response expected"));
    };
    if settings.revision > 0 {
        if settings.selection.as_ref() == Some(selected)
            && settings.effective.as_ref() == Some(selected)
        {
            return Ok(());
        }
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "installed Host network changed",
        ));
    }
    let mut request = Request::new(
        target,
        Command::SaveNetworkSettings {
            expected_revision: 0,
            selection: selected.clone(),
        },
    );
    request.id = id;
    let admission = remote.dispatch(request).await?;
    let Output::NetworkSettings(saved) = admission
        .completion
        .await
        .map_err(|_| unknown("network change response was lost"))??
    else {
        return Err(Fault::invalid("network settings response expected"));
    };
    let actual: sailry_link::EndpointAddr = serde_json::from_str(&saved.address)
        .map_err(|_| Fault::invalid("invalid Host network address"))?;
    if actual.id.as_bytes() != &target.0 {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "network settings belong to another Host",
        ));
    }
    link.update_peer(actual).await?;
    let admission = remote.dispatch(read()).await?;
    let Output::NetworkSettings(verified) = admission
        .completion
        .await
        .map_err(|_| unknown("network verification response was lost"))??
    else {
        return Err(Fault::invalid("network settings response expected"));
    };
    if verified.revision == 0
        || verified.selection.as_ref() != Some(selected)
        || verified.effective.as_ref() != Some(selected)
    {
        return Err(unknown("Host network change could not be confirmed"));
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
        Outcome::Completed { exit_code: 73, .. } => Err(unknown(
            "Host installer ran but completion could not be confirmed; inspect the Host before retrying",
        )),
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
    #[cfg(unix)]
    fn preserves_installer_arguments() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("unexpected");
        let urls = vec![
            format!(
                "https://relay.example/path/'\"$x%20;$(touch {})",
                marker.display()
            ),
            "https://backup.example:8443".into(),
        ];
        let custom = Selection::Custom {
            relays: urls.clone(),
        };
        for (selected, expected) in [
            (None, Vec::<String>::new()),
            (Some(Selection::Iroh), vec!["--internet".into()]),
            (
                Some(custom),
                urls.into_iter()
                    .flat_map(|url| ["--relay".into(), url])
                    .collect(),
            ),
        ] {
            let source = script("0.1.0-alpha.1", false, selected.as_ref()).unwrap();
            let prefix = source
                .strip_suffix(include_str!("install/release.sh"))
                .unwrap();
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(format!(
                    "{prefix}\nfor argument do printf '%s\\0' \"$argument\"; done"
                ))
                .output()
                .unwrap();
            assert!(output.status.success());
            let arguments = output
                .stdout
                .split(|byte| *byte == 0)
                .filter(|value| !value.is_empty())
                .map(|value| String::from_utf8(value.to_vec()).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(arguments, expected);
            assert!(!marker.exists());
        }
        assert!(
            script(
                "0.1.0-alpha.1",
                false,
                Some(&Selection::Custom {
                    relays: vec!["https://relay.example/\0".into()]
                })
            )
            .is_err()
        );
    }

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

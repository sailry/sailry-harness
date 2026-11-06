use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
#[cfg(unix)]
use sailry_protocol::wsl::Distribution;
use sailry_protocol::{host::installation::Outcome, network::Selection, *};

#[cfg(unix)]
mod contracts {
    use super::*;
    use sha2::Digest as _;
    use std::{os::unix::fs::PermissionsExt as _, path::Path, time::Duration};

    const NAME: &str = "Ubuntu 资料";

    fn executable(path: &Path, source: &str) {
        std::fs::write(path, source).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn fixture(root: &Path, node: &Node, ticket: &str, mode: &str, valid: bool) {
        std::fs::create_dir_all(root.join(".fixture/bin")).unwrap();
        std::fs::create_dir_all(root.join(".fixture/assets")).unwrap();
        std::fs::create_dir(root.join("tmp")).unwrap();
        std::fs::write(root.join("mode"), mode).unwrap();
        std::fs::write(root.join("bootstrap-source"), ticket).unwrap();
        executable(&root.join("wsl"), include_str!("wsl/driver.sh"));
        executable(
            &root.join(".fixture/bin/curl"),
            include_str!("ssh/install/download.sh"),
        );
        let installer = format!(
            r#"#!/bin/sh
set -eu
test "$1" = --version
test "$2" = '{}'
printf installed >> "$HOME/install-calls"
if test "$(cat "$HOME/mode")" = wait-install; then sleep 30; fi
mkdir "$HOME/.sailry-host"
cp "$HOME/bootstrap-source" "$HOME/.sailry-host/bootstrap.ticket"
"#,
            env!("SAILRY_HOST_VERSION")
        );
        executable(&root.join(".fixture/assets/install-host.sh"), &installer);
        let digest = if valid {
            format!("{:x}", sha2::Sha256::digest(installer.as_bytes()))
        } else {
            "0".repeat(64)
        };
        std::fs::write(
            root.join(".fixture/assets/SHA256SUMS-installer"),
            format!("{digest}  install-host.sh\n"),
        )
        .unwrap();
        node.set_wsl_fixture(
            root.join("wsl"),
            vec![
                Distribution {
                    name: NAME.into(),
                    version: 2,
                },
                Distribution {
                    name: "Legacy".into(),
                    version: 1,
                },
            ],
        );
    }

    async fn controller(node: &Node, root: &Path, remote: bool) -> (Link, Client) {
        let controller = Link::controller(root.join("controller"), NetworkScope::default())
            .await
            .unwrap();
        let invitation = node.link().invite().unwrap();
        let address = controller.handle().pair(invitation.ticket()).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        (controller, client)
    }

    fn install(client: &Client, network: Option<Selection>) -> Request {
        client.prepare(Command::InstallWslHost {
            distribution: NAME.into(),
            network,
        })
    }

    async fn marker(path: &Path) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while !path.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("WSL fixture did not reach its expected stage");
    }

    #[tokio::test]
    async fn deploys_and_pairs_without_ssh_profiles() {
        for (remote, network) in [
            (false, None),
            (true, None),
            (false, Some(Selection::Iroh)),
            (true, Some(Selection::Iroh)),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let node = Node::start_empty(directory.path().join("node"))
                .await
                .unwrap();
            let target = Node::start_empty(directory.path().join("target"))
                .await
                .unwrap();
            let invitation = target.link().invite().unwrap();
            fixture(directory.path(), &node, invitation.ticket(), "ready", true);
            let (controller, client) = controller(&node, directory.path(), remote).await;
            let listing = client
                .dispatch(client.prepare(Command::ListWslDistributions))
                .await
                .unwrap();
            assert!(!listing.receipt.durable);
            assert_eq!(
                listing.completion.await.unwrap().unwrap(),
                Output::WslDistributions(vec![
                    Distribution {
                        name: NAME.into(),
                        version: 2
                    },
                    Distribution {
                        name: "Legacy".into(),
                        version: 1
                    }
                ])
            );
            let request = install(&client, network.clone());
            let admission = client.dispatch(request.clone()).await.unwrap();
            assert!(admission.receipt.durable);
            let output = admission.completion.await.unwrap().unwrap();
            let expected = match network {
                None => Outcome::Installed { node: target.id() },
                Some(selection) => Outcome::NetworkUnconfirmed {
                    node: target.id(),
                    selection,
                },
            };
            assert_eq!(output, Output::HostInstallation(expected));
            assert_ne!(node.id(), target.id());
            assert_eq!(client.execute(request.clone()).await.unwrap(), output);
            assert_eq!(
                client.outcome(&request).await.unwrap(),
                RequestOutcome::Completed(Box::new(Ok(output)))
            );
            assert_eq!(
                std::fs::read(directory.path().join("install-calls")).unwrap(),
                b"installed"
            );
            assert_eq!(
                client
                    .execute(client.prepare(Command::ListSsh))
                    .await
                    .unwrap(),
                Output::SshProfiles(vec![])
            );
            let address = node
                .link()
                .peers()
                .await
                .unwrap()
                .into_iter()
                .find(|address| address.id.as_bytes() == &target.id().0)
                .unwrap();
            let target_client = Client::new(node.link().remote(address));
            assert!(matches!(
                target_client
                    .execute(target_client.prepare(Command::InspectHost))
                    .await
                    .unwrap(),
                Output::HostInfo(_)
            ));
            controller.close().await.unwrap();
            target.shutdown().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }

    #[tokio::test]
    async fn rejects_uninstalled_or_unsupported_targets() {
        for remote in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let node = Node::start_empty(directory.path().join("node"))
                .await
                .unwrap();
            fixture(directory.path(), &node, "unused", "ready", true);
            let (controller, client) = controller(&node, directory.path(), remote).await;
            for (distribution, code) in [
                ("/bin/bash", ErrorCode::NotFound),
                ("Missing", ErrorCode::NotFound),
                ("Legacy", ErrorCode::Unavailable),
                ("--install", ErrorCode::InvalidRequest),
                ("Ubuntu\nDebian", ErrorCode::InvalidRequest),
            ] {
                let request = client.prepare(Command::InstallWslHost {
                    distribution: distribution.into(),
                    network: None,
                });
                let error = client.execute(request.clone()).await.unwrap_err();
                assert_eq!(error.code, code);
                assert_eq!(client.execute(request).await.unwrap_err(), error);
            }
            assert_eq!(
                client
                    .execute(install(&client, Some(Selection::Custom { relays: vec![] })))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
            assert!(!directory.path().join("detecting").exists());
            assert!(!directory.path().join("install-calls").exists());
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }

    #[tokio::test]
    async fn preserves_unverified_and_existing_installations() {
        for remote in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let node = Node::start_empty(directory.path().join("node"))
                .await
                .unwrap();
            fixture(directory.path(), &node, "unused", "ready", false);
            let (controller, client) = controller(&node, directory.path(), remote).await;
            let request = install(&client, None);
            let error = client.execute(request.clone()).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::Unavailable);
            assert!(error.message.contains("Installer checksum does not match"));
            assert_eq!(client.execute(request).await.unwrap_err(), error);
            assert!(!directory.path().join("install-calls").exists());
            std::fs::create_dir(directory.path().join(".sailry-host")).unwrap();
            let marker = directory.path().join(".sailry-host/preserve");
            std::fs::write(&marker, b"original").unwrap();
            let error = client.execute(install(&client, None)).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::Unavailable);
            assert!(error.message.contains("already exist"));
            assert_eq!(std::fs::read(marker).unwrap(), b"original");
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }

    #[tokio::test]
    async fn cancels_with_honest_durable_outcomes() {
        for (remote, mode, stage, code) in [
            (
                false,
                "wait-detect",
                ssh::InstallProgress::Detecting,
                ErrorCode::Cancelled,
            ),
            (
                true,
                "wait-detect",
                ssh::InstallProgress::Detecting,
                ErrorCode::Cancelled,
            ),
            (
                false,
                "wait-install",
                ssh::InstallProgress::Installing,
                ErrorCode::OutcomeUnknown,
            ),
            (
                true,
                "wait-install",
                ssh::InstallProgress::Installing,
                ErrorCode::OutcomeUnknown,
            ),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let node = Node::start_empty(directory.path().join("node"))
                .await
                .unwrap();
            fixture(directory.path(), &node, "unused", mode, true);
            let (controller, client) = controller(&node, directory.path(), remote).await;
            let request = install(&client, None);
            let admission = client.dispatch(request.clone()).await.unwrap();
            assert!(admission.receipt.durable);
            marker(&directory.path().join(if mode == "wait-detect" {
                "detecting"
            } else {
                "install-calls"
            }))
            .await;
            assert_eq!(
                client
                    .execute(client.prepare(Command::ReadWslHostInstall {
                        request: request.id
                    }))
                    .await
                    .unwrap(),
                Output::HostInstallProgress(stage)
            );
            let other = Link::controller(
                directory.path().join("other-controller"),
                NetworkScope::default(),
            )
            .await
            .unwrap();
            let invitation = node.link().invite().unwrap();
            let address = other.handle().pair(invitation.ticket()).await.unwrap();
            let outsider = Client::new(other.handle().remote(address));
            assert_eq!(
                outsider
                    .execute(outsider.prepare(Command::ReadWslHostInstall {
                        request: request.id
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
            assert_eq!(
                outsider
                    .execute(outsider.prepare(Command::CancelWslHostInstall {
                        request: request.id
                    }))
                    .await
                    .unwrap(),
                Output::WslHostInstallCancelled {
                    request: request.id
                }
            );
            assert!(
                client
                    .execute(client.prepare(Command::ReadWslHostInstall {
                        request: request.id
                    }))
                    .await
                    .is_ok()
            );
            other.close().await.unwrap();
            let conflict = client.execute(install(&client, None)).await.unwrap_err();
            assert_eq!(conflict.code, ErrorCode::Conflict);
            let cancel = client
                .dispatch(client.prepare(Command::CancelWslHostInstall {
                    request: request.id,
                }))
                .await
                .unwrap();
            assert!(cancel.receipt.durable);
            assert_eq!(
                cancel.completion.await.unwrap().unwrap(),
                Output::WslHostInstallCancelled {
                    request: request.id
                }
            );
            let error = tokio::time::timeout(Duration::from_secs(10), admission.completion)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err();
            assert_eq!(error.code, code);
            assert_eq!(client.execute(request).await.unwrap_err(), error);
            assert!(!directory.path().join(".sailry-host").exists());
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "requires an explicitly selected empty WSL2 distribution, systemd and the matching published Host release"]
async fn deploys_installed_distribution() {
    let distribution = std::env::var("SAILRY_TEST_WSL_DISTRIBUTION")
        .expect("Select an empty installed WSL2 distribution");
    let directory = tempfile::tempdir().unwrap();
    let node =
        Node::start_empty_with_network(directory.path().join("node"), NetworkScope::Internet)
            .await
            .unwrap();
    let listing = Client::new(node.local());
    let Output::WslDistributions(installed) = listing
        .execute(listing.prepare(Command::ListWslDistributions))
        .await
        .unwrap()
    else {
        panic!("WSL distribution list expected");
    };
    assert!(
        installed
            .iter()
            .any(|entry| entry.name == distribution && entry.version == 2)
    );
    let remote = std::env::var("SAILRY_TEST_WSL_REMOTE").is_ok_and(|value| value == "1");
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::Internet)
        .await
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let address = controller.handle().pair(invitation.ticket()).await.unwrap();
    let client = Client::new(if remote {
        controller.handle().remote(address)
    } else {
        node.local()
    });
    let request = client.prepare(Command::InstallWslHost {
        distribution,
        network: Some(Selection::Iroh),
    });
    let admission = client.dispatch(request.clone()).await.unwrap();
    assert!(admission.receipt.durable);
    let Output::HostInstallation(Outcome::Installed { node: installed }) =
        admission.completion.await.unwrap().unwrap()
    else {
        panic!("Confirmed independent Host expected");
    };
    assert_ne!(installed, node.id());
    assert_eq!(
        client.execute(request).await.unwrap(),
        Output::HostInstallation(Outcome::Installed { node: installed })
    );
    let address = node
        .link()
        .peers()
        .await
        .unwrap()
        .into_iter()
        .find(|address| address.id.as_bytes() == &installed.0)
        .unwrap();
    let host = Client::new(node.link().remote(address));
    let Output::HostInfo(info) = host
        .execute(host.prepare(Command::InspectHost))
        .await
        .unwrap()
    else {
        panic!("Independent Linux Host expected");
    };
    assert_eq!(info.node, installed);
    assert!(
        info.os
            .as_ref()
            .is_some_and(|os| !os.to_ascii_lowercase().contains("windows"))
    );
    assert_eq!(
        client
            .execute(client.prepare(Command::ListSsh))
            .await
            .unwrap(),
        Output::SshProfiles(vec![])
    );
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

use super::*;

fn downloads(root: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let bin = root.join(".fixture/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(root.join(".fixture/assets")).unwrap();
    let curl = bin.join("curl");
    std::fs::write(&curl, include_str!("install/download.sh")).unwrap();
    std::fs::set_permissions(curl, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[tokio::test]
async fn rejects_unverified_installer_without_replay() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        downloads(directory.path());
        std::fs::write(
            directory.path().join(".fixture/assets/install-host.sh"),
            "#!/bin/sh\nprintf forbidden > \"$HOME/installer-executed\"\n",
        )
        .unwrap();
        std::fs::write(
            directory
                .path()
                .join(".fixture/assets/SHA256SUMS-installer"),
            format!("{}  install-host.sh\n", "0".repeat(64)),
        )
        .unwrap();
        let server = server::Server::deployment(directory.path().into(), 47).await;
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let invitation = node.link().invite().unwrap();
        let address = controller.handle().pair(invitation.ticket()).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let profile = save(
            &client,
            profile(server.port),
            Some(Credential::Password {
                password: Secret::new("isolated-ssh-password".into()),
            }),
        )
        .await;
        let profile = trust(&client, &profile, &server.key).await;
        let request = client.prepare(Command::InstallHost {
            profile: profile.id,
            expected_revision: profile.revision,
        });
        let fault = client.execute(request.clone()).await.unwrap_err();
        assert_eq!(fault.code, ErrorCode::Unavailable);
        assert!(fault.message.contains("Installer checksum does not match"));
        assert_eq!(client.execute(request).await.unwrap_err(), fault);
        assert!(!directory.path().join("installer-executed").exists());
        assert!(!directory.path().join(".sailry-host").exists());
        let commands = server.commands.lock().unwrap();
        let installation = commands
            .iter()
            .filter(|command| command.contains("releases/download/v"))
            .collect::<Vec<_>>();
        assert_eq!(installation.len(), 1);
        assert!(installation[0].contains(&format!(
            "install_version='{}'",
            env!("SAILRY_HOST_VERSION")
        )));
        assert!(!commands.iter().any(|command| command.contains("cat >")));
        drop(commands);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        server.close().await;
    }
}

#[tokio::test]
#[cfg(target_os = "macos")]
#[ignore = "requires SAILRY_TEST_HOST_RELEASE, Internet, and a launchd GUI session"]
async fn deploys_and_pairs() {
    for remote in [false, true] {
        let directory = tempfile::Builder::new()
            .prefix("sailry-install-")
            .tempdir()
            .unwrap();
        downloads(directory.path());
        release(directory.path());
        let server = server::Server::deployment(directory.path().into(), 46).await;
        let node =
            Node::start_with_network(directory.path().join("desktop"), NetworkScope::Internet)
                .await
                .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::Internet)
                .await
                .unwrap();
        let invitation = node.link().invite().unwrap();
        let address = controller.handle().pair(invitation.ticket()).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let profile = save(
            &client,
            profile(server.port),
            Some(Credential::Password {
                password: Secret::new("isolated-ssh-password".into()),
            }),
        )
        .await;
        let blocked = execute(
            &client,
            Command::InstallHost {
                profile: profile.id,
                expected_revision: profile.revision,
            },
        )
        .await;
        assert!(matches!(
            blocked,
            Output::SshOutcome(Outcome::HostKeyRequired { .. })
        ));
        assert_eq!(server.authentication.load(Ordering::SeqCst), 0);
        let profile = trust(&client, &profile, &server.key).await;
        let request = client.prepare(Command::InstallHost {
            profile: profile.id,
            expected_revision: profile.revision,
        });
        let install = client.execute(request.clone());
        tokio::pin!(install);
        let mut stages = Vec::new();
        let uid = std::process::Command::new("id").arg("-u").output().unwrap();
        let target = format!(
            "gui/{}/ai.sailry.host.acceptance.{}",
            String::from_utf8_lossy(&uid.stdout).trim(),
            directory.path().file_name().unwrap().to_str().unwrap()
        );
        struct Service(String);
        impl Drop for Service {
            fn drop(&mut self) {
                let _ = std::process::Command::new("launchctl")
                    .args(["bootout", &self.0])
                    .output();
            }
        }
        let service = Service(target);
        let output = tokio::time::timeout(Duration::from_secs(180), async {
            loop {
                tokio::select! {
                    output = &mut install => break output.unwrap(),
                    _ = tokio::time::sleep(Duration::from_millis(20)) => {
                        if let Ok(Output::HostInstallProgress(progress)) = client.execute(client.prepare(Command::ReadHostInstall { request: request.id })).await { stages.push(progress); }
                    }
                }
            }
        }).await.unwrap();
        let Output::SshOutcome(Outcome::HostInstalled { node: installed }) = output else {
            panic!("installed host expected")
        };
        assert!(
            stages
                .iter()
                .any(|stage| matches!(stage, ssh::InstallProgress::Installing))
        );
        assert!(
            stages
                .iter()
                .any(|stage| matches!(stage, ssh::InstallProgress::Pairing))
        );
        assert!(
            !directory
                .path()
                .join(".sailry-host/bootstrap.ticket")
                .exists()
        );
        let address = node
            .link()
            .peers()
            .await
            .unwrap()
            .into_iter()
            .find(|peer| NodeId(*peer.id.as_bytes()) == installed)
            .unwrap();
        let host = Client::new(node.link().remote(address));
        assert!(matches!(
            execute(&host, Command::InspectHost).await,
            Output::HostInfo(_)
        ));
        let project = directory.path().join("office-project");
        std::fs::create_dir(&project).unwrap();
        execute(
            &host,
            Command::RegisterProject {
                name: "Office deployment fixture".into(),
                path: project.to_str().unwrap().into(),
            },
        )
        .await;
        let Output::Snapshot(snapshot) = execute(&host, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let Output::OfficeRuntime(runtime) = execute(
            &host,
            Command::OfficeRuntime {
                worktree: snapshot.worktrees[0].id,
            },
        )
        .await
        else {
            panic!("deployed Office runtime expected")
        };
        assert!(
            std::path::Path::new(&runtime.python).starts_with(
                directory
                    .path()
                    .join(".local/lib/sailry/office-runtime")
                    .canonicalize()
                    .unwrap()
            )
        );
        let verify = format!(
            "'{}' -c 'import docx, openpyxl, pptx, reportlab, pypdf, pdfplumber; print(\"office-ready\")'",
            runtime.python.replace('\'', "'\"'\"'")
        );
        let verify = Command::RunSsh {
            profile: profile.id,
            expected_revision: profile.revision,
            command: verify,
            timeout_ms: 30000,
        };
        assert!(matches!(
            execute(&client, verify).await,
            Output::SshOutcome(Outcome::Completed { exit_code: 0, stdout, .. })
                if stdout.trim() == "office-ready"
        ));
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::SshOutcome(Outcome::HostInstalled { node: installed })
        );
        // Durable completion prevents another download or service installation.
        assert_eq!(
            server
                .commands
                .lock()
                .unwrap()
                .iter()
                .filter(|command| command.contains("releases/download/v"))
                .count(),
            1
        );
        drop(service);
        tokio::time::sleep(Duration::from_millis(500)).await;
        server.close().await;
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[cfg(target_os = "macos")]
fn release(root: &std::path::Path) {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::PermissionsExt;
    let package = std::path::PathBuf::from(
        std::env::var_os("SAILRY_TEST_HOST_RELEASE")
            .expect("Native Host release fixture is required"),
    );
    let target = if cfg!(target_arch = "aarch64") {
        "aarch64-apple-darwin"
    } else {
        "x86_64-apple-darwin"
    };
    let archive = format!(
        "sailry-host-{}-{target}.tar.gz",
        env!("SAILRY_HOST_VERSION")
    );
    let assets = root.join(".fixture/assets");
    std::os::unix::fs::symlink(package.join(&archive), assets.join(&archive)).unwrap();
    let checksums = std::fs::read_to_string(package.join("SHA256SUMS")).unwrap();
    let checksum = checksums
        .lines()
        .find(|line| line.ends_with(&archive))
        .unwrap();
    std::fs::write(
        assets.join(format!("SHA256SUMS-{target}")),
        format!("{checksum}\n"),
    )
    .unwrap();
    let installer = include_bytes!("../../../../scripts/install-host.sh");
    std::fs::write(assets.join("install-host.sh"), installer).unwrap();
    std::fs::write(
        assets.join("SHA256SUMS-installer"),
        format!("{:x}  install-host.sh\n", Sha256::digest(installer)),
    )
    .unwrap();
    // Only namespace the launchd label: binary, installer and archive bytes remain unchanged.
    let label = format!(
        "ai.sailry.host.acceptance.{}",
        root.file_name().unwrap().to_str().unwrap()
    );
    let launchctl = root.join(".fixture/bin/launchctl");
    std::fs::write(
        &launchctl,
        format!(
            r#"#!/usr/bin/env bash
set -eu
if test "$1" = bootstrap && test "$3" = "$HOME/Library/LaunchAgents/ai.sailry.host.plist"; then
  /usr/libexec/PlistBuddy -c 'Set :Label {label}' "$3"
fi
exec /bin/launchctl "$@"
"#
        ),
    )
    .unwrap();
    std::fs::set_permissions(launchctl, std::fs::Permissions::from_mode(0o755)).unwrap();
}

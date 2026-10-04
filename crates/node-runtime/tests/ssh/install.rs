use super::*;

#[tokio::test]
#[cfg(target_os = "macos")]
#[ignore = "requires a bundled native Host binary, Internet, and a launchd GUI session"]
async fn deploys_and_pairs() {
    for remote in [false, true] {
        let directory = tempfile::Builder::new()
            .prefix("sailry-install-")
            .tempdir()
            .unwrap();
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
                .any(|stage| matches!(stage, ssh::InstallProgress::Uploading { .. }))
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
        // Durable completion prevents a second upload or service installation.
        assert_eq!(
            server
                .commands
                .lock()
                .unwrap()
                .iter()
                .filter(|command| command.contains("cat >") && command.contains("sailry-host.new"))
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

use super::*;

fn query() -> Query {
    Query::Hover {
        path: "main.rs".into(),
        line: 1,
        symbol: "symbol".into(),
    }
}

async fn start(client: &Client, worktree: WorktreeId) -> code::Startup {
    let Output::Code(result) = client
        .execute(client.prepare(Command::ReadCode {
            worktree,
            query: query(),
            overlay: None,
        }))
        .await
        .unwrap()
    else {
        panic!("code expected")
    };
    let startup = result.startup.unwrap();
    assert!(matches!(
        client
            .execute(client.prepare(Command::StartCodeServer {
                startup: startup.clone()
            }))
            .await
            .unwrap(),
        Output::CodeServer(_)
    ));
    startup
}

fn running(pid: &str) -> bool {
    let output = std::process::Command::new("ps")
        .args(["-p", pid, "-o", "stat="])
        .output()
        .unwrap();
    let state = String::from_utf8(output.stdout).unwrap();
    let state = state.trim();
    !state.is_empty() && !state.starts_with('Z')
}

#[tokio::test]
async fn stops_owned_group_without_changing_settings() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let fixture = Fixture::new(remote, &server).await;
        let other_root = fixture.directory.path().join("other");
        std::fs::create_dir(&other_root).unwrap();
        std::fs::copy(
            fixture.root.join("Cargo.toml"),
            other_root.join("Cargo.toml"),
        )
        .unwrap();
        std::fs::copy(fixture.root.join("main.rs"), other_root.join("main.rs")).unwrap();
        let Output::Project(project) = fixture
            .client
            .execute(fixture.client.prepare(Command::RegisterProject {
                name: "Other worktree".into(),
                path: other_root.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Session(other) = fixture
            .client
            .execute(fixture.client.prepare(Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(fixture.session.config.clone()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let other_startup = start(&fixture.client, other.worktree).await;
        let other_pid = std::fs::read_to_string(other_root.join("pid")).unwrap();
        std::fs::write(fixture.root.join("control"), br#"{"child":true}"#).unwrap();
        let startup = start(&fixture.client, fixture.session.worktree).await;
        let pid = std::fs::read_to_string(fixture.root.join("pid")).unwrap();
        let child = std::fs::read_to_string(fixture.root.join("child_pid")).unwrap();
        let before = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadCodeSettings))
            .await
            .unwrap();
        let mut events = fixture
            .client
            .subscribe_code_servers(fixture.session.worktree)
            .await
            .unwrap();
        assert!(
            matches!(events.next().await.unwrap(), sailry_protocol::Update::CodeServers { servers, .. } if servers.len() == 1)
        );
        std::fs::write(fixture.root.join("control"), br#"{"hang_shutdown":true}"#).unwrap();
        let request = fixture.client.prepare(Command::StopCodeServer {
            startup: startup.clone(),
        });
        let admitted = fixture.client.dispatch(request.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        let output = admitted.completion.await.unwrap().unwrap();
        assert!(
            matches!(&output, Output::CodeServer(stopped) if stopped.startup == startup && stopped.status == code::Status::Stopped && stopped.progress.is_none())
        );
        assert!(!running(&pid));
        assert!(!running(&child));
        assert!(running(&other_pid));
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(3), events.next())
                .await
                .unwrap()
                .unwrap(),
            sailry_protocol::Update::CodeServers { worktree, servers, .. }
                if worktree == fixture.session.worktree && servers.is_empty()
        ));
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadCodeSettings))
                .await
                .unwrap(),
            before
        );
        let cold = fixture.read(query(), None).await;
        assert_eq!(cold.status, code::Status::Stopped);
        assert_eq!(cold.startup.as_ref(), Some(&startup));
        assert!(cold.startup_approved);
        std::fs::remove_file(fixture.root.join("control")).unwrap();
        start(&fixture.client, fixture.session.worktree).await;
        let restarted = std::fs::read_to_string(fixture.root.join("pid")).unwrap();
        assert_ne!(restarted, pid);
        assert_eq!(
            fixture.client.execute(request.clone()).await.unwrap(),
            output
        );
        assert!(running(&restarted));
        let mut conflicting = request;
        let Command::StopCodeServer { startup } = &mut conflicting.command else {
            unreachable!()
        };
        startup.revision = "0".repeat(64);
        assert_eq!(
            fixture.client.execute(conflicting).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        assert!(running(&restarted));
        assert!(matches!(
            fixture.client.execute(fixture.client.prepare(Command::ListCodeServers {
                worktree: other.worktree
            })).await.unwrap(),
            Output::CodeServers(servers) if servers.len() == 1 && servers[0].startup == other_startup
        ));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_stale_and_plugin_descriptors() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let fixture = Fixture::new(remote, &server).await;
        let original = start(&fixture.client, fixture.session.worktree).await;
        fixture.settings(vec!["changed".into()], false).await;
        let current = start(&fixture.client, fixture.session.worktree).await;
        assert_ne!(current.revision, original.revision);
        let pid = std::fs::read_to_string(fixture.root.join("pid")).unwrap();
        let mut executable = current.clone();
        executable.executable = "/bin/sh".into();
        let mut traversal = current.clone();
        traversal.root = "../outside".into();
        let mut worktree = current.clone();
        worktree.worktree = WorktreeId::new();
        for (startup, expected) in [
            (original, ErrorCode::RevisionConflict),
            (executable, ErrorCode::RevisionConflict),
            (traversal, ErrorCode::InvalidRequest),
            (worktree, ErrorCode::NotFound),
        ] {
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::StopCodeServer { startup }))
                    .await
                    .unwrap_err()
                    .code,
                expected
            );
            assert!(running(&pid));
        }
        let Output::Plugin(info) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadPlugin {
                name: "code".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("plugin expected")
        };
        let context = sailry_protocol::plugin::Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: info.summary.reference(),
            worktree: Some(fixture.session.worktree),
            session: Some(fixture.session.id),
        };
        assert_eq!(
            fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::StopCodeServer {
                            startup: current.clone()
                        })
                        .with_plugin(context)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert!(running(&pid));
        fixture
            .client
            .execute(fixture.client.prepare(Command::StopCodeServer {
                startup: current.clone(),
            }))
            .await
            .unwrap();
        assert_eq!(
            fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::StopCodeServer { startup: current })
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn disabling_service_stops_all_worktrees() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let fixture = Fixture::new(remote, &server).await;
        fixture.settings(Vec::new(), false).await;
        let other_root = fixture.directory.path().join("other");
        std::fs::create_dir(&other_root).unwrap();
        for file in ["Cargo.toml", "main.rs"] {
            std::fs::copy(fixture.root.join(file), other_root.join(file)).unwrap();
        }
        let Output::Project(project) = fixture
            .client
            .execute(fixture.client.prepare(Command::RegisterProject {
                name: "Other worktree".into(),
                path: other_root.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Session(other) = fixture
            .client
            .execute(fixture.client.prepare(Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(fixture.session.config.clone()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let startups = [
            start(&fixture.client, fixture.session.worktree).await,
            start(&fixture.client, other.worktree).await,
        ];
        let pids = [&fixture.root, &other_root]
            .map(|root| std::fs::read_to_string(root.join("pid")).unwrap());
        assert!(pids.iter().all(|pid| running(pid)));
        let Output::CodeSettings(mut settings) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadCodeSettings))
            .await
            .unwrap()
        else {
            panic!("settings expected")
        };
        settings.servers.get_mut("rust-analyzer").unwrap().disabled = true;
        let Output::CodeSettings(mut settings) = fixture
            .client
            .execute(fixture.client.prepare(Command::SaveCodeSettings {
                expected_revision: settings.revision,
                settings,
            }))
            .await
            .unwrap()
        else {
            panic!("settings expected")
        };
        assert!(settings.enabled);
        assert!(
            pids.iter().all(|pid| !running(pid)),
            "completion waits for every owned instance to stop"
        );
        for startup in &startups {
            let Output::CodeServers(servers) = fixture
                .client
                .execute(fixture.client.prepare(Command::ListCodeServers {
                    worktree: startup.worktree,
                }))
                .await
                .unwrap()
            else {
                panic!("servers expected")
            };
            assert!(servers.is_empty());
            assert!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::StartCodeServer {
                        startup: startup.clone()
                    }))
                    .await
                    .is_err()
            );
            let Output::Code(result) = fixture
                .client
                .execute(fixture.client.prepare(Command::ReadCode {
                    worktree: startup.worktree,
                    query: query(),
                    overlay: None,
                }))
                .await
                .unwrap()
            else {
                panic!("code expected")
            };
            assert!(result.startup.is_none());
        }
        settings.servers.get_mut("rust-analyzer").unwrap().disabled = false;
        fixture
            .client
            .execute(fixture.client.prepare(Command::SaveCodeSettings {
                expected_revision: settings.revision,
                settings,
            }))
            .await
            .unwrap();
        // Re-enabling changes eligibility, without eagerly launching either project.
        assert!(pids.iter().all(|pid| !running(pid)));
        for startup in startups {
            assert_eq!(start(&fixture.client, startup.worktree).await, startup);
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

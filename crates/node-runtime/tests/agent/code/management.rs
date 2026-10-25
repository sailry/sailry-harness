use super::*;

fn query() -> Query {
    Query::Hover {
        path: "main.rs".into(),
        line: 1,
        symbol: "symbol".into(),
    }
}

async fn installed(fixture: &Fixture) -> Vec<code::Installation> {
    let Output::CodeInstallations(items) = fixture
        .client
        .execute(fixture.client.prepare(Command::ListCodeInstallations))
        .await
        .unwrap()
    else {
        panic!("installations expected")
    };
    items
}

#[tokio::test]
async fn recovers_missing_proxy_components() {
    use std::os::unix::fs::PermissionsExt;
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![]).await;
        let sources = install::Sources::new().await;
        let mut fixture = Fixture::new(remote, &server).await;
        let launcher = fixture.bin.join("rustup");
        std::fs::write(
            &launcher,
            "#!/bin/sh\nprintf ran > proxy-started\nexit 92\n",
        )
        .unwrap();
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
        let executable = fixture.bin.join("rust-analyzer");
        std::fs::remove_file(&executable).unwrap();
        std::os::unix::fs::symlink(&launcher, &executable).unwrap();
        let rustup = fixture.directory.path().join(".rustup");
        std::fs::create_dir_all(&rustup).unwrap();
        std::fs::write(
            rustup.join("settings.toml"),
            "default_toolchain = 'stable-x86_64-unknown-linux-gnu'\n",
        )
        .unwrap();
        std::fs::write(
            fixture.root.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = '1.98.0'\nprofile = 'minimal'\n",
        )
        .unwrap();
        fixture = install::restart(fixture, remote, &sources, None).await;
        fixture.config(WorkMode::Code, Permission::Full).await;
        let Output::CodeSettings(mut settings) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadCodeSettings))
            .await
            .unwrap()
        else {
            panic!("settings expected")
        };
        settings.editor_enabled = true;
        fixture
            .client
            .execute(fixture.client.prepare(Command::SaveCodeSettings {
                expected_revision: settings.revision,
                settings,
            }))
            .await
            .unwrap();
        let missing = fixture.read(query(), None).await;
        assert_eq!(missing.status, code::Status::NotInstalled);
        assert!(missing.startup.is_none());
        assert_eq!(missing.package.as_deref(), Some("rust-analyzer"));
        let descriptor = missing.package_info.unwrap().install.unwrap();
        assert!(!fixture.root.join("proxy-started").exists());
        assert!(installed(&fixture).await.is_empty());
        server.replace_tools(vec![
            (
                plugin_tool("code", "code_hover"),
                json!({"path":"main.rs","line":1,"symbol":"symbol"}),
            ),
            (
                plugin_tool("code", "code_install"),
                serde_json::to_value(&descriptor).unwrap(),
            ),
            (
                plugin_tool("code", "code_diagnostics"),
                json!({"path":"main.rs"}),
            ),
        ]);
        let turn = fixture.submit().await;
        let page = fixture.finish(turn).await;
        let output = results(&page);
        assert_eq!(output[0]["status"], "not_installed", "{output:?}");
        assert_eq!(
            output[0]["package_info"]["install"],
            serde_json::to_value(&descriptor).unwrap()
        );
        assert!(
            output.iter().any(|result| {
                result["status"] == "ready" && result.to_string().contains("fixture syntax error")
            }),
            "{output:?}"
        );
        assert!(!fixture.root.join("proxy-started").exists());
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("main.rs")).unwrap(),
            "🙂 broken symbol\n"
        );
        let Output::CodeSettings(settings) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadCodeSettings))
            .await
            .unwrap()
        else {
            panic!("settings expected")
        };
        assert!(settings.enabled && settings.editor_enabled);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn installs_queries_and_uninstalls_through_agent() {
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![]).await;
        let sources = install::Sources::new().await;
        let mut fixture = Fixture::new(remote, &server).await;
        std::fs::remove_file(fixture.bin.join("rust-analyzer")).unwrap();
        fixture = install::restart(fixture, remote, &sources, None).await;
        fixture.config(WorkMode::Code, Permission::Full).await;
        let descriptor = fixture
            .read(query(), None)
            .await
            .package_info
            .unwrap()
            .install
            .unwrap();
        server.replace_tools(vec![
            (
                plugin_tool("code", "code_packages"),
                json!({"installed":false}),
            ),
            (
                plugin_tool("code", "code_install"),
                serde_json::to_value(&descriptor).unwrap(),
            ),
            (
                plugin_tool("code", "code_hover"),
                json!({"path":"main.rs","line":1,"symbol":"symbol"}),
            ),
            (
                plugin_tool("code", "code_packages"),
                json!({"installed":true}),
            ),
        ]);
        let turn = fixture.submit().await;
        let page = fixture.finish(turn).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn)
                .unwrap()
                .status,
            Status::Completed
        );
        let results = results(&page);
        assert!(
            results
                .iter()
                .any(|result| result.to_string().contains("fixture signature")),
            "{results:?}"
        );
        let items = installed(&fixture).await;
        let receipt = items.iter().find_map(|item| item.receipt.as_ref()).unwrap();
        assert_eq!(receipt.revision, descriptor.revision);
        assert!(receipt.downloads.iter().all(|download| download.verified));
        assert!(
            page.approvals
                .iter()
                .filter(|approval| approval.turn == turn)
                .all(|approval| approval.source == ApprovalSource::Full)
        );
        server.replace_tools(vec![(
            plugin_tool("code", "code_uninstall"),
            json!({"package":receipt.package,"version":receipt.version}),
        )]);
        let turn = fixture.submit().await;
        let page = fixture.finish(turn).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn)
                .unwrap()
                .status,
            Status::Completed
        );
        assert!(
            installed(&fixture)
                .await
                .iter()
                .all(|item| item.receipt.is_none())
        );
        let Output::CodeServers(servers) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListCodeServers {
                worktree: fixture.session.worktree,
            }))
            .await
            .unwrap()
        else {
            panic!("servers expected")
        };
        assert!(servers.is_empty());
        assert_eq!(
            fixture.read(query(), None).await.status,
            code::Status::NotInstalled
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn requires_approval_and_preserves_denial() {
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![]).await;
        let sources = install::Sources::new().await;
        let mut fixture = Fixture::new(remote, &server).await;
        std::fs::remove_file(fixture.bin.join("rust-analyzer")).unwrap();
        fixture = install::restart(fixture, remote, &sources, None).await;
        fixture.config(WorkMode::Code, Permission::Ask).await;
        let descriptor = fixture
            .read(query(), None)
            .await
            .package_info
            .unwrap()
            .install
            .unwrap();
        server.replace_tools(vec![(
            plugin_tool("code", "code_install"),
            serde_json::to_value(&descriptor).unwrap(),
        )]);
        let turn = fixture.submit().await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert_eq!(approval.source, ApprovalSource::User);
        assert!(installed(&fixture).await.is_empty());
        process::decide(
            &fixture.client,
            &approval,
            Decision::DenyWithReason("Keep existing software".into()),
        )
        .await;
        let page = fixture.finish(turn).await;
        assert!(
            results(&page)
                .iter()
                .any(|result| result.to_string().contains("Keep existing software"))
        );
        assert!(installed(&fixture).await.is_empty());
        let turn = fixture.submit().await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        process::decide(&fixture.client, &approval, Decision::Approve).await;
        fixture.finish(turn).await;
        assert!(
            installed(&fixture)
                .await
                .iter()
                .any(|item| item.receipt.is_some())
        );
        server.replace_tools(vec![(
            plugin_tool("code", "code_uninstall"),
            json!({"package":descriptor.package,"version":descriptor.version}),
        )]);
        let turn = fixture.submit().await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert!(
            installed(&fixture)
                .await
                .iter()
                .any(|item| item.receipt.is_some())
        );
        process::decide(&fixture.client, &approval, Decision::Deny).await;
        fixture.finish(turn).await;
        assert!(
            installed(&fixture)
                .await
                .iter()
                .any(|item| item.receipt.is_some())
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn stopping_turn_cancels_dependency_download() {
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![]).await;
        let sources = install::Sources::new().await;
        sources
            .held
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let mut fixture = Fixture::new(remote, &server).await;
        std::fs::remove_file(fixture.bin.join("rust-analyzer")).unwrap();
        fixture = install::restart(fixture, remote, &sources, None).await;
        fixture.config(WorkMode::Code, Permission::Full).await;
        let descriptor = fixture
            .read(query(), None)
            .await
            .package_info
            .unwrap()
            .install
            .unwrap();
        server.replace_tools(vec![(
            plugin_tool("code", "code_install"),
            serde_json::to_value(&descriptor).unwrap(),
        )]);
        let turn = fixture.submit().await;
        tokio::time::timeout(Duration::from_secs(15), async {
            while installed(&fixture).await.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        fixture
            .client
            .execute(fixture.client.prepare(Command::StopTurn { turn }))
            .await
            .unwrap();
        fixture.finish(turn).await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let items = installed(&fixture).await;
                assert!(items.iter().all(|item| item.receipt.is_none()));
                if items
                    .iter()
                    .any(|item| item.status == code::InstallStatus::Cancelled)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

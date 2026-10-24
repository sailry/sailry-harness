#![cfg(any(target_os = "macos", target_os = "linux"))]
use super::*;

fn settings(
    fixture: &Fixture,
    executable: &std::path::Path,
    address: std::net::SocketAddr,
    dependency: Option<&std::path::Path>,
) {
    let value = json!({"sandbox":{
        "writes":{
            "outside":fixture.directory.path().join("denied"),
            "program":executable.parent().unwrap().join("denied"),
            "profile":fixture.node.profile().join("denied")
        },
        "readonly":executable,
        "protected":fixture.node.profile().join("storage/node.turso"),
        "network":[address.ip().to_string(),address.port()],
        "dependency":dependency
    }});
    std::fs::write(
        fixture.root.join("control"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
}

async fn confined(fixture: &Fixture, listener: &tokio::net::TcpListener, dependency: bool) {
    let result: Value =
        serde_json::from_slice(&std::fs::read(fixture.root.join("sandbox")).unwrap()).unwrap();
    for name in ["parent", "child"] {
        let mut expected = json!({"outside":false,"program":false,"profile":false,"readonly":false,"protected":false,"network":false});
        if dependency {
            expected["dependency"] = json!(true);
        }
        assert_eq!(result[name], expected);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(format!("{name}-allowed.txt"))).unwrap(),
            "workspace"
        );
        assert!(
            !fixture
                .directory
                .path()
                .join(format!("denied-{name}"))
                .exists()
        );
        assert!(
            !fixture
                .node
                .profile()
                .join(format!("denied-{name}"))
                .exists()
        );
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn confines_servers_and_children_after_once_approval() {
    for remote in [false, true] {
        for permission in [Permission::Ask, Permission::Project, Permission::Full] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let server = Server::turn_tools(vec![(
                plugin_tool("code", "code_hover"),
                json!({"path":"main.rs","line":1,"symbol":"symbol"}),
            )])
            .await;
            let mut fixture = Fixture::new(remote, &server).await;
            fixture.config(WorkMode::Code, permission).await;
            let executable = fixture.bin.join("rust-analyzer");
            let original = std::fs::read(&executable).unwrap();
            settings(&fixture, &executable, listener.local_addr().unwrap(), None);
            let turn = fixture.submit().await;
            if permission == Permission::Ask {
                let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
                assert!(approval.startup.is_some());
                assert_eq!(approval.source, ApprovalSource::User);
                assert!(!fixture.root.join("pid").exists());
                process::decide(&fixture.client, &approval, Decision::Approve).await;
            }
            let page = fixture.finish(turn).await;
            assert!(
                results(&page)
                    .iter()
                    .any(|result| result.to_string().contains("fixture signature"))
            );
            confined(&fixture, &listener, false).await;
            assert_eq!(std::fs::read(&executable).unwrap(), original);
            let pid = std::fs::read_to_string(fixture.root.join("pid")).unwrap();
            let turn = fixture.submit().await;
            let page = fixture.finish(turn).await;
            assert!(
                results(&page)
                    .iter()
                    .any(|result| result.to_string().contains("fixture signature"))
            );
            assert!(
                !page
                    .approvals
                    .iter()
                    .any(|approval| approval.turn == turn
                        && approval.state == ApprovalState::Pending)
            );
            assert_eq!(
                std::fs::read_to_string(fixture.root.join("pid")).unwrap(),
                pid
            );
            confined(&fixture, &listener, false).await;
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn reads_only_the_selected_installed_package() {
    for remote in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server = Server::tools(vec![]).await;
        let sources = install::Sources::new().await;
        let mut fixture = Fixture::new(remote, &server).await;
        std::fs::remove_file(fixture.bin.join("rust-analyzer")).unwrap();
        fixture = install::restart(fixture, remote, &sources, None).await;
        let query = Query::Diagnostics {
            path: "main.rs".into(),
            severity: None,
        };
        let cold = fixture.read(query.clone(), None).await;
        let install = cold.package_info.unwrap().install.unwrap();
        fixture
            .client
            .execute(fixture.client.prepare(Command::InstallCodeServer {
                install: install.clone(),
            }))
            .await
            .unwrap();
        let cold = fixture.read(query.clone(), None).await;
        let startup = cold.startup.unwrap();
        let executable = PathBuf::from(&startup.executable);
        let original = std::fs::read(&executable).unwrap();
        let dependency = executable
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("dependency.json");
        std::fs::write(&dependency, "package dependency").unwrap();
        settings(
            &fixture,
            &executable,
            listener.local_addr().unwrap(),
            Some(&dependency),
        );
        fixture
            .client
            .execute(fixture.client.prepare(Command::StartCodeServer { startup }))
            .await
            .unwrap();
        let Content::Diagnostics(items) = fixture.read(query, None).await.content else {
            panic!("diagnostics expected")
        };
        assert_eq!(items.len(), 1);
        confined(&fixture, &listener, true).await;
        assert_eq!(std::fs::read(&executable).unwrap(), original);
        fixture
            .client
            .execute(fixture.client.prepare(Command::UninstallCodeServer {
                package: install.package,
                version: install.version,
            }))
            .await
            .unwrap();
        assert!(!executable.exists());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

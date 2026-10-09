#![cfg(any(target_os = "macos", target_os = "linux"))]
use super::*;
use process::Fixture;
use sailry_protocol::{
    conversation::ApprovalChoice,
    process::{Access, Outcome},
};
use serde_json::json;
use std::time::Duration;

fn quote(path: &std::path::Path) -> String {
    format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
}

async fn permission(fixture: &mut Fixture, permission: Permission) {
    let mut config = fixture.session.config.clone();
    config.permission = permission;
    let Output::Session(session) = fixture
        .client
        .execute(fixture.client.prepare(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config,
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    fixture.session = session;
}

#[tokio::test]
async fn confines_default_commands_under_every_permission() {
    for remote in [false, true] {
        for mode in [Permission::Ask, Permission::Project, Permission::Full] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let curl = which::which("curl").expect("sandbox acceptance requires curl");
            let server = Server::tools(vec![
                (plugin_tool("commands", "run_command"), json!({"command":"printf workspace > allowed.txt; if printf outside > ../denied.txt; then exit 91; fi; printf confined"})),
                (plugin_tool("commands", "run_command"), json!({"command":format!("if {} --silent --max-time 1 http://{address}/ > network.txt; then exit 92; fi; printf offline", quote(&curl))})),
                (plugin_tool("commands", "run_command"), json!({"command":"printf ready > background.txt; sleep 60", "background":true})),
            ]).await;
            let mut fixture = Fixture::new(remote, &server).await;
            permission(&mut fixture, mode).await;
            let turn = approvals::rules::submit(&fixture.client, &fixture.session).await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            let outputs = process::results(&page);
            assert_eq!(outputs.len(), 2, "{page:?}");
            assert!(
                outputs
                    .iter()
                    .all(|output| output.outcome == Outcome::Exited(0))
            );
            assert_eq!(outputs[0].stdout.text, "confined");
            assert_eq!(outputs[1].stdout.text, "offline");
            assert_eq!(
                std::fs::read_to_string(fixture.root.join("allowed.txt")).unwrap(),
                "workspace"
            );
            assert!(!fixture.directory.path().join("denied.txt").exists());
            assert!(
                tokio::time::timeout(Duration::from_millis(50), listener.accept())
                    .await
                    .is_err()
            );
            assert_eq!(page.approvals.len(), 3);
            for approval in &page.approvals {
                assert_eq!(approval.source, ApprovalSource::Sandbox);
                assert_eq!(approval.state, ApprovalState::Approved);
                assert_eq!(approval.access, Some(Access::Workspace));
                assert!(!approval.can_remember);
                assert_eq!(approval.prefix, None);
                assert!(approvals::rules::claimed(&fixture.node, approval).1);
            }
            let Output::Commands(commands) = fixture
                .client
                .execute(fixture.client.prepare(Command::ListCommands {
                    session: fixture.session.id,
                }))
                .await
                .unwrap()
            else {
                panic!("commands expected")
            };
            assert_eq!(commands.len(), 1);
            assert_eq!(commands[0].access, Access::Workspace);
            assert_eq!(commands[0].turn, turn.id);
            assert_eq!(
                commands[0].status,
                sailry_protocol::process::Status::Running
            );
            fixture
                .client
                .execute(fixture.client.prepare(Command::StopCommand {
                    session: fixture.session.id,
                    id: commands[0].id,
                }))
                .await
                .unwrap();
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn escape_waits_for_fresh_consent_under_full() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for remote in [false, true] {
        for mode in [Permission::Ask, Permission::Project, Permission::Full] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let peer = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = [0; 2048];
                assert!(stream.read(&mut bytes).await.unwrap() > 0);
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nallowed",
                    )
                    .await
                    .unwrap();
            });
            let curl = which::which("curl").expect("sandbox acceptance requires curl");
            let server = Server::tools(vec![
                (plugin_tool("commands", "run_command"), json!({"access":"unrestricted","command":format!("{} --silent --max-time 3 http://{address}/ && printf outside > ../allowed.txt", quote(&curl))})),
                (plugin_tool("commands", "run_command"), json!({"access":"unrestricted","command":"touch ../denied.txt"})),
            ]).await;
            let mut fixture = Fixture::new(remote, &server).await;
            permission(&mut fixture, mode).await;
            let turn = approvals::rules::submit(&fixture.client, &fixture.session).await;
            let (_, first) = approvals::pending(&fixture.client, fixture.session.id).await;
            assert_eq!(first.source, ApprovalSource::User);
            assert_eq!(first.access, Some(Access::Unrestricted));
            assert!(!first.can_remember);
            assert_eq!(first.prefix, None);
            assert!(!fixture.directory.path().join("allowed.txt").exists());
            assert!(!peer.is_finished());
            process::decide(&fixture.client, &first, Decision::Approve).await;
            let (_, second) = approvals::pending(&fixture.client, fixture.session.id).await;
            assert_ne!(first.id, second.id);
            assert_eq!(second.source, ApprovalSource::User);
            assert!(!fixture.directory.path().join("denied.txt").exists());
            process::decide(
                &fixture.client,
                &second,
                Decision::DenyWithReason("Stay within this workspace".into()),
            )
            .await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            tokio::time::timeout(Duration::from_secs(5), peer)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                std::fs::read_to_string(fixture.directory.path().join("allowed.txt")).unwrap(),
                "outside"
            );
            assert!(!fixture.directory.path().join("denied.txt").exists());
            assert_eq!(process::results(&page).len(), 1);
            assert_eq!(process::results(&page)[0].stdout.text, "allowed");
            assert_eq!(process::results(&page)[0].outcome, Outcome::Exited(0));
            assert_eq!(page.approvals[0].choice, Some(ApprovalChoice::Once));
            assert_eq!(
                page.approvals[1].reason.as_deref(),
                Some("Stay within this workspace")
            );
            assert!(approvals::rules::claimed(&fixture.node, &first).1);
            assert!(!approvals::rules::claimed(&fixture.node, &second).1);
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn rejects_outside_working_directory() {
    for remote in [false, true] {
        let server = Server::tools(vec![(
            plugin_tool("commands", "run_command"),
            json!({"command":"touch escaped.txt","cwd":".."}),
        )])
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let turn = approvals::submit(&fixture.client, fixture.session.id).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert!(!fixture.directory.path().join("escaped.txt").exists());
        let result = approvals::rules::results(&page, turn).pop().unwrap().2;
        assert_eq!(result["isError"], true);
        assert_eq!(result["error"]["code"], "permission_denied");
        assert!(process::results(&page).is_empty());
        assert_eq!(page.approvals[0].source, ApprovalSource::Sandbox);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

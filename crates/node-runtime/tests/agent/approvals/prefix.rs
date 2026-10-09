#![cfg(unix)]
use super::rules::{claimed, completed, pending, restart, submit};
use super::*;
use process::Fixture;
use sailry_protocol::{conversation::ApprovalChoice, process::Access};
use serde_json::json;

#[tokio::test]
async fn escape_rejects_remembering_after_restart() {
    for remote in [false, true] {
        let server = Server::turn_tools(vec![(
            plugin_tool("commands", "run_command"),
            json!({"access":"unrestricted","command":"touch first.txt"}),
        )])
        .await;
        let mut fixture = Fixture::new(remote, &server).await;
        let first = submit(&fixture.client, &fixture.session).await;
        let approval = pending(&fixture.client, fixture.session.id).await;
        assert_eq!(approval.access, Some(Access::Unrestricted));
        assert_eq!(approval.source, ApprovalSource::User);
        assert_eq!(approval.prefix, None);
        assert!(!approval.can_remember);
        for decision in [
            Decision::ApproveSession,
            Decision::ApprovePrefix("touch".into()),
            Decision::ApprovePrefix("touch first.txt".into()),
        ] {
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::ResolveApproval {
                        session: fixture.session.id,
                        approval: approval.id,
                        decision,
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
            assert_eq!(pending(&fixture.client, fixture.session.id).await, approval);
            assert!(!fixture.root.join("first.txt").exists());
            assert!(!claimed(&fixture.node, &approval).1);
        }
        process::decide(&fixture.client, &approval, Decision::Approve).await;
        let page = completed(&fixture.client, fixture.session.id, first.id).await;
        assert_eq!(page.approvals[0].choice, Some(ApprovalChoice::Once));
        assert!(fixture.root.join("first.txt").exists());
        fixture = restart(fixture, remote).await;
        let next = Server::turn_tools(vec![(plugin_tool("commands", "run_command"),
            json!({"access":"unrestricted","command":"touch second.txt","description":"Create the next fixture"}))]).await;
        super::rules::route(&fixture.client, &fixture.session, &next.endpoint).await;
        let turn = submit(&fixture.client, &fixture.session).await;
        let approval = pending(&fixture.client, fixture.session.id).await;
        assert_eq!(approval.source, ApprovalSource::User);
        assert_eq!(approval.access, Some(Access::Unrestricted));
        assert!(!fixture.root.join("second.txt").exists());
        let page = history(&fixture.client, fixture.session.id).await;
        let entry = page
            .entries
            .iter()
            .find(|entry| entry.id == approval.entry)
            .unwrap();
        let Part::ToolCall {
            name, arguments, ..
        } = &entry.parts[approval.index]
        else {
            panic!("canonical tool call expected")
        };
        assert_eq!(name, &plugin_tool("commands", "run_command"));
        assert_eq!(
            arguments,
            &json!({"access":"unrestricted","command":"touch second.txt","description":"Create the next fixture"})
        );
        process::decide(&fixture.client, &approval, Decision::Deny).await;
        let page = completed(&fixture.client, fixture.session.id, turn.id).await;
        assert!(!claimed(&fixture.node, &approval).1);
        assert!(!fixture.root.join("second.txt").exists());
        assert!(page.approvals.iter().all(|approval| !matches!(
            approval.source,
            ApprovalSource::SessionRule | ApprovalSource::PrefixRule
        )));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn composite_escapes_always_require_fresh_consent() {
    for remote in [false, true] {
        let commands = [
            "touchy denied.txt",
            "touch denied.txt; touch other.txt",
            "touch denied.txt && touch other.txt",
            "touch denied.txt | cat",
            "touch denied.txt > output.txt",
            "touch $(printf denied.txt)",
            "touch `printf denied.txt`",
            "touch 'denied.txt'",
            "touch denied\\.txt",
            "FLAG=x touch denied.txt",
            "touch denied.txt\ntouch other.txt",
        ];
        let mut calls = vec![(
            plugin_tool("commands", "run_command"),
            json!({"access":"unrestricted","command":"touch seed.txt"}),
        )];
        calls.extend(commands.iter().map(|command| {
            (
                plugin_tool("commands", "run_command"),
                json!({"access":"unrestricted","command":command}),
            )
        }));
        let server = Server::tools(calls).await;
        let fixture = Fixture::new(remote, &server).await;
        let turn = submit(&fixture.client, &fixture.session).await;
        let first = pending(&fixture.client, fixture.session.id).await;
        process::decide(&fixture.client, &first, Decision::Approve).await;
        let mut seen = std::collections::BTreeSet::from([first.id]);
        for _ in commands {
            let approval = pending(&fixture.client, fixture.session.id).await;
            assert!(seen.insert(approval.id));
            assert_eq!(approval.source, ApprovalSource::User);
            assert_eq!(approval.access, Some(Access::Unrestricted));
            assert_eq!(approval.prefix, None);
            assert!(!approval.can_remember);
            for name in ["denied.txt", "other.txt", "output.txt"] {
                assert!(!fixture.root.join(name).exists());
            }
            process::decide(&fixture.client, &approval, Decision::Deny).await;
        }
        let page = completed(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.approvals.len(), commands.len() + 1);
        assert!(fixture.root.join("seed.txt").exists());
        assert!(
            page.approvals
                .iter()
                .all(|approval| approval.source == ApprovalSource::User)
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

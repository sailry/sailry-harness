#![cfg(unix)]
use super::rules::{claimed, completed, pending, restart, submit};
use super::*;
use process::Fixture;
use sailry_protocol::conversation::ApprovalChoice;
use serde_json::json;

#[tokio::test]
async fn reuses_literal_words_after_restart() {
    for remote in [false, true] {
        let server = Server::turn_tools(vec![(
            plugin_tool("commands", "run_command"),
            json!({"command":"touch first.txt"}),
        )])
        .await;
        let mut fixture = Fixture::new(remote, &server).await;
        let first = submit(&fixture.client, &fixture.session).await;
        let approval = pending(&fixture.client, fixture.session.id).await;
        assert_eq!(approval.prefix.as_deref(), Some("touch first.txt"));
        for prefix in [
            "",
            "tou",
            "touch first",
            "touch first.txt other",
            "touch;",
            "touch\n",
            "touch\0",
        ] {
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::ResolveApproval {
                        session: fixture.session.id,
                        approval: approval.id,
                        decision: Decision::ApprovePrefix(prefix.into()),
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
            assert_eq!(pending(&fixture.client, fixture.session.id).await, approval);
            assert!(!fixture.root.join("first.txt").exists());
        }
        process::decide(
            &fixture.client,
            &approval,
            Decision::ApprovePrefix("touch".into()),
        )
        .await;
        let page = completed(&fixture.client, fixture.session.id, first.id).await;
        assert_eq!(
            page.approvals[0].choice,
            Some(ApprovalChoice::Prefix("touch".into()))
        );
        assert!(fixture.root.join("first.txt").exists());
        fixture = restart(fixture, remote).await;
        let next = Server::turn_tools(vec![
            (
                plugin_tool("commands", "run_command"),
                json!({"command":"  touch   second.txt  "}),
            ),
            (
                plugin_tool("commands", "run_command"),
                json!({"command":"mkdir denied","description":"touch approved-prefix"}),
            ),
        ])
        .await;
        super::rules::route(&fixture.client, &fixture.session, &next.endpoint).await;
        let turn = submit(&fixture.client, &fixture.session).await;
        let approval = pending(&fixture.client, fixture.session.id).await;
        assert_eq!(approval.source, ApprovalSource::User);
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
            &json!({"command":"mkdir denied","description":"touch approved-prefix"})
        );
        assert!(fixture.root.join("second.txt").exists());
        assert!(!fixture.root.join("denied").exists());
        process::decide(&fixture.client, &approval, Decision::Deny).await;
        let page = completed(&fixture.client, fixture.session.id, turn.id).await;
        let allowed = page
            .approvals
            .iter()
            .find(|approval| {
                approval.turn == turn.id && approval.source == ApprovalSource::PrefixRule
            })
            .unwrap();
        assert!(claimed(&fixture.node, allowed).1);
        assert_ne!(
            claimed(&fixture.node, allowed).0,
            claimed(&fixture.node, &page.approvals[0]).0
        );
        assert!(!claimed(&fixture.node, &approval).1);
        assert!(!fixture.root.join("denied").exists());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn excludes_composites_and_word_extensions() {
    for remote in [false, true] {
        let mut calls = vec![(
            plugin_tool("commands", "run_command"),
            json!({"command":"touch seed.txt"}),
        )];
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
        calls.extend(commands.iter().map(|command| {
            (
                plugin_tool("commands", "run_command"),
                json!({"command":command}),
            )
        }));
        let server = Server::tools(calls).await;
        let fixture = Fixture::new(remote, &server).await;
        let turn = submit(&fixture.client, &fixture.session).await;
        let first = pending(&fixture.client, fixture.session.id).await;
        process::decide(
            &fixture.client,
            &first,
            Decision::ApprovePrefix("touch".into()),
        )
        .await;
        let mut seen = std::collections::BTreeSet::from([first.id]);
        for _ in commands {
            let approval = pending(&fixture.client, fixture.session.id).await;
            assert!(seen.insert(approval.id));
            assert_eq!(approval.source, ApprovalSource::User);
            assert!(!fixture.root.join("denied.txt").exists());
            assert!(!fixture.root.join("other.txt").exists());
            assert!(!fixture.root.join("output.txt").exists());
            process::decide(&fixture.client, &approval, Decision::Deny).await;
        }
        let page = completed(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.approvals.len(), commands.len() + 1);
        assert!(fixture.root.join("seed.txt").exists());
        assert!(!fixture.root.join("denied.txt").exists());
        assert!(
            page.approvals
                .iter()
                .all(|approval| approval.source != ApprovalSource::PrefixRule)
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

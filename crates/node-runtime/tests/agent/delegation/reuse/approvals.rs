use super::super::fixture::Fixture;
use super::super::*;

fn assert_call(page: &Page, approval: &Approval) {
    let entry = page
        .entries
        .iter()
        .find(|entry| entry.id == approval.entry && entry.turn == approval.turn)
        .unwrap();
    assert!(matches!(entry.parts.get(approval.index),
        Some(Part::ToolCall { id: Some(id), name, .. })
            if id == "call-fixture-0" && name == &plugin_tool("files", "write_file")
    ));
}

#[tokio::test]
async fn isolates_repeated_call_identity() {
    for remote in [false, true] {
        let parent = Server::turn_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role": "review", "task": "Initial approved write"}),
        )])
        .await;
        let child = Server::turn_tools(vec![(
            plugin_tool("files", "write_file"),
            json!({"path": "written.txt", "text": "Approved once 中文 🙂", "expected_revision": null}),
        )])
        .await;
        let fixture = Fixture::start(remote, parent, child).await;
        let first_parent = fixture.push("Run the initial review").await;
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        let (page, first) = crate::approvals::pending(&fixture.client, child.id).await;
        assert_call(&page, &first);
        assert_eq!(first.session, child.id);
        assert!(!fixture.root().join("written.txt").exists());
        let resolution = fixture.client.prepare(Command::ResolveApproval {
            session: child.id,
            approval: first.id,
            decision: Decision::Approve,
        });
        let accepted = fixture.client.execute(resolution.clone()).await.unwrap();
        let Output::Approval(approved) = &accepted else {
            panic!("approval expected")
        };
        assert_eq!(approved.id, first.id);
        assert_eq!(approved.turn, first.turn);
        assert_eq!(approved.state, ApprovalState::Approved);
        let original = finished(&fixture.client, fixture.session.id, first_parent.id).await;
        assert_eq!(original.children.len(), 1);
        assert_eq!(original.children[0].run.turn, first.turn);
        assert_eq!(original.children[0].run.status, Status::Completed);
        assert_eq!(
            std::fs::read_to_string(fixture.root().join("written.txt")).unwrap(),
            "Approved once 中文 🙂"
        );
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 2);

        let next_parent = Server::turn_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"session": child.id, "task": "Second denied write"}),
        )])
        .await;
        fixture.route_parent(&next_parent.endpoint).await;
        let second_parent = fixture.push("Reuse the existing review").await;
        assert_eq!(second_parent.config, first_parent.config);
        assert_eq!(second_parent.roles, first_parent.roles);
        let (page, second) = crate::approvals::pending(&fixture.client, child.id).await;
        assert_call(&page, &second);
        assert_eq!(second.session, child.id);
        assert_ne!(second.id, first.id);
        assert_ne!(second.turn, first.turn);
        assert_ne!(second.entry, first.entry);
        assert_eq!(second.state, ApprovalState::Pending);
        assert_eq!(page.runs.len(), 2);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 3);

        // Matching decisions on closed approvals remain idempotent, but neither
        // replay nor a fresh command for the old ID can authorize this new turn.
        assert_eq!(fixture.client.execute(resolution).await.unwrap(), accepted);
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ResolveApproval {
                    session: child.id,
                    approval: first.id,
                    decision: Decision::Approve,
                }))
                .await
                .unwrap(),
            accepted
        );
        let (page, pending) = crate::approvals::pending(&fixture.client, child.id).await;
        assert_eq!(pending, second);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 3);
        assert_eq!(
            page.approvals
                .iter()
                .find(|approval| approval.id == first.id)
                .unwrap()
                .state,
            ApprovalState::Approved
        );
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveApproval {
                session: child.id,
                approval: second.id,
                decision: Decision::Deny,
            }))
            .await
            .unwrap();
        let parent = finished(&fixture.client, fixture.session.id, second_parent.id).await;
        let page = finished(&fixture.client, child.id, second.turn).await;
        assert_eq!(page.runs.len(), 2);
        assert!(page.runs.iter().all(|run| run.status == Status::Completed));
        assert!(page.queue.items.is_empty());
        assert_eq!(page.approvals.len(), 2);
        for (approval, state) in [
            (&first, ApprovalState::Approved),
            (&second, ApprovalState::Denied),
        ] {
            let retained = page
                .approvals
                .iter()
                .find(|retained| retained.id == approval.id)
                .unwrap();
            assert_eq!(retained.turn, approval.turn);
            assert_eq!(retained.state, state);
            assert_call(&page, retained);
        }
        assert_eq!(parent.children.len(), 2);
        assert_eq!(parent.children[0], original.children[0]);
        for (parent_turn, child_turn) in [
            (first_parent.id, first.turn),
            (second_parent.id, second.turn),
        ] {
            assert_eq!(
                parent
                    .runs
                    .iter()
                    .find(|run| run.turn == parent_turn)
                    .unwrap()
                    .status,
                Status::Completed
            );
            let summary = parent
                .children
                .iter()
                .find(|summary| summary.origin.turn == parent_turn)
                .unwrap();
            assert_eq!(summary.run.session, child.id);
            assert_eq!(summary.run.turn, child_turn);
            assert_eq!(
                &summary.run,
                page.runs.iter().find(|run| run.turn == child_turn).unwrap()
            );
            assert_eq!(summary.origin.session, fixture.session.id);
            assert_eq!(summary.origin.role, Some(fixture.role.id));
            let entry = parent
                .entries
                .iter()
                .find(|entry| entry.id == summary.origin.entry && entry.turn == parent_turn)
                .unwrap();
            assert!(matches!(entry.parts.get(summary.origin.index),
                Some(Part::ToolCall { id: Some(id), name, .. })
                    if id == "call-fixture-0" && name == &plugin_tool("delegation", "spawn_agent")
            ));
            let mut page = parent.clone();
            page.entries.retain(|entry| entry.turn == parent_turn);
            let returned = results(&fixture.client, &page).await;
            assert_eq!(returned.len(), 1);
            assert_eq!(returned[0]["session"], json!(child.id));
            assert_eq!(returned[0]["turn"], json!(child_turn));
            assert_eq!(returned[0]["status"], "completed");
        }
        assert_ne!(
            parent.children[0].origin.entry,
            parent.children[1].origin.entry
        );
        let snapshot = fixture.snapshot().await;
        let parent = snapshot
            .sessions
            .iter()
            .find(|session| session.id == fixture.session.id)
            .unwrap();
        assert_eq!(parent.project, fixture.session.project);
        assert_eq!(parent.worktree, fixture.session.worktree);
        assert_eq!(parent.revision, fixture.session.revision);
        assert_eq!(parent.config, fixture.session.config);
        assert_eq!(parent.roles, fixture.session.roles);
        assert_eq!(parent.profile, fixture.session.profile);
        assert_eq!(parent.delegation, fixture.session.delegation);
        let sessions: Vec<_> = snapshot
            .sessions
            .iter()
            .filter(|session| {
                session
                    .delegation
                    .as_ref()
                    .is_some_and(|origin| origin.session == fixture.session.id)
            })
            .collect();
        assert_eq!(sessions.len(), 1);
        let retained = sessions[0];
        assert_eq!(retained.id, child.id);
        assert_eq!(retained.project, child.project);
        assert_eq!(retained.worktree, child.worktree);
        assert_eq!(retained.revision, child.revision);
        assert_eq!(retained.config, child.config);
        assert_eq!(retained.roles, child.roles);
        assert_eq!(retained.profile, child.profile);
        assert_eq!(retained.delegation, child.delegation);
        assert_eq!(
            child.delegation.as_deref(),
            Some(&original.children[0].origin)
        );
        assert_eq!(
            snapshot
                .turns
                .iter()
                .filter(|turn| turn.session == child.id)
                .count(),
            2
        );
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 4);
        assert_eq!(fixture.parent.requests.lock().unwrap().len(), 2);
        assert_eq!(next_parent.requests.lock().unwrap().len(), 2);
        assert_eq!(
            std::fs::read_to_string(fixture.root().join("written.txt")).unwrap(),
            "Approved once 中文 🙂"
        );
        fixture.close().await;
    }
}

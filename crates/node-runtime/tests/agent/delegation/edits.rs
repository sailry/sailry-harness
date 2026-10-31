use super::*;

#[tokio::test]
async fn projects_child_edit_diff() {
    for remote in [false, true] {
        let original = "Child original\n";
        let parent = Server::turn_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"review","task":"Edit the note"}),
        )])
        .await;
        let child = Server::turn_tools(vec![(
            plugin_tool("files", "edit_file"),
            json!({
                "path":"note.txt","old_text":"Child original","new_text":"Child edited",
                "expected_revision":blake3::hash(original.as_bytes()).to_hex().to_string()
            }),
        )])
        .await;
        let fixture = fixture::Fixture::start(remote, parent, child).await;
        std::fs::write(fixture.root().join("note.txt"), original).unwrap();
        let turn = fixture.push("Delegate the edit").await;
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        let approval = crate::approvals::rules::pending(&fixture.client, child.id).await;
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveApproval {
                session: child.id,
                approval: approval.id,
                decision: Decision::Approve,
            }))
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        let page = finished(&fixture.client, child.id, approval.turn).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert_eq!(
            std::fs::read_to_string(fixture.root().join("note.txt")).unwrap(),
            "Child edited\n"
        );
        let details = tool_details(&fixture.client, &page).await;
        assert_eq!(details.len(), 1);
        let output = details[0]
            .1
            .resolved
            .as_ref()
            .and_then(|resolved| resolved.output.as_ref())
            .expect("resolved output expected");
        assert!(output.preview.is_empty() && output.notices.is_empty());
        let body = output.body.as_ref().expect("diff body expected");
        assert_eq!(
            body.diff,
            Some(sailry_protocol::tool::projection::Format::Unified)
        );
        assert_eq!(body.path.as_deref(), Some("note.txt"));
        assert!(
            body.text.contains("-Child original\n") && body.text.contains("+Child edited\n"),
            "{}",
            body.text
        );
        fixture.close().await;
    }
}

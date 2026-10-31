//! A saved file snapshot projects its confirmed diff, whatever the captured display declares.
use super::*;
use sailry_protocol::tool::{
    detail::{Body, Detail},
    projection::Format,
};

fn shown(detail: &Detail) -> &sailry_protocol::tool::detail::Output {
    detail
        .resolved
        .as_ref()
        .and_then(|resolved| resolved.output.as_ref())
        .expect("resolved output expected")
}

/// The diff body, which replaces fragment previews and result captions.
fn unified(detail: &Detail) -> &Body {
    let output = shown(detail);
    assert!(output.preview.is_empty(), "{output:?}");
    assert!(output.notices.is_empty(), "{output:?}");
    let body = output.body.as_ref().expect("diff body expected");
    assert_eq!(body.diff, Some(Format::Unified), "{body:?}");
    body
}

fn hunks(body: &Body) -> usize {
    body.text
        .lines()
        .filter(|line| line.starts_with("@@ "))
        .count()
}

/// Rows after the first hunk header.
fn hunk_rows(body: &Body) -> Vec<&str> {
    body.text
        .lines()
        .skip_while(|line| !line.starts_with("@@ "))
        .skip(1)
        .collect()
}

/// Result notices that collapsed rows still carry from the captured display.
fn captions(page: &Page) -> usize {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| match part {
            Part::ToolResult { summary, .. } => summary.resolved.as_ref()?.output.as_ref(),
            _ => None,
        })
        .map(|output| output.notices.len())
        .sum()
}

async fn prepare(remote: bool, server: &Server, files: &[(&str, &str)]) -> process::Fixture {
    let fixture = process::Fixture::packages(remote, server, &[]).await;
    install(&fixture).await;
    for (path, text) in files {
        std::fs::write(fixture.root.join(path), text).unwrap();
    }
    fixture
}

async fn finish(fixture: &mut process::Fixture) -> (TurnId, Page) {
    configure(fixture, Permission::Project, WorkMode::Code).await;
    let turn = submit(fixture).await;
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{page:?}"
    );
    (turn.id, page)
}

/// Rewrites the captured display of `tool` as files 0.1.17 declared it: no diff
/// and a "Written" notice after the output.
async fn freeze_caption(node: &Node, turn: TurnId, tool: String) {
    let pointer = format!("/metadata/presentation/displays/{tool}");
    node.with_storage(move |db| {
        Box::pin(async move {
            let mut rows = db
                .prepare("SELECT sequence,body FROM agent_events WHERE turn=?1")
                .await
                .map_err(crate::database_fixture::fault)?
                .query([turn.to_string()])
                .await
                .map_err(crate::database_fixture::fault)?;
            let mut updates = Vec::new();
            while let Some(row) = rows.next().await.map_err(crate::database_fixture::fault)? {
                let sequence: i64 = row.get(0).map_err(crate::database_fixture::fault)?;
                let body: Vec<u8> = row.get(1).map_err(crate::database_fixture::fault)?;
                let mut event: Value = serde_json::from_slice(&body).unwrap();
                let Some(display) = event.pointer_mut(&pointer) else {
                    continue;
                };
                let output = display["output"]
                    .as_object_mut()
                    .expect("output recipe expected");
                output
                    .get_mut("body")
                    .and_then(Value::as_object_mut)
                    .expect("body recipe expected")
                    .remove("diff");
                output.insert("notices".into(), json!([{"message":{"label":"Written"}}]));
                updates.push((sequence, serde_json::to_vec(&event).unwrap()));
            }
            drop(rows);
            assert!(!updates.is_empty(), "captured display expected");
            for (sequence, body) in updates {
                db.execute(
                    "UPDATE agent_events SET body=?1 WHERE sequence=?2",
                    turso::params![body, sequence],
                )
                .await
                .map_err(crate::database_fixture::fault)?;
            }
            Ok(())
        })
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn projects_multi_edit_hunks() {
    for remote in [false, true] {
        let fillers: String = (1..=12)
            .map(|number| format!("filler {number}\n"))
            .collect();
        let original = format!("alpha\n{fillers}omega\n");
        let server = Server::tools(vec![(
            name("edit_file"),
            json!({
                "path":"note.txt",
                "expected_revision":blake3::hash(original.as_bytes()).to_hex().to_string(),
                "edits":[
                    {"old_text":"alpha","new_text":"first"},
                    {"old_text":"omega","new_text":"last"}
                ]
            }),
        )])
        .await;
        let mut fixture = prepare(remote, &server, &[("note.txt", original.as_str())]).await;
        let (_, page) = finish(&mut fixture).await;
        let details = tool_details(&fixture.client, &page).await;
        assert_eq!(details.len(), 1);
        let body = unified(&details[0].1);
        assert_eq!(body.path.as_deref(), Some("note.txt"));
        assert_eq!(hunks(body), 2, "{}", body.text);
        for row in ["-alpha\n", "+first\n", "-omega\n", "+last\n"] {
            assert!(body.text.contains(row), "{}", body.text);
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn projects_created_file_as_added_rows() {
    for remote in [false, true] {
        let server = Server::tools(vec![(
            name("write_file"),
            json!({"path":"created.txt","text":"One\nTwo\nThree\n","expected_revision":null}),
        )])
        .await;
        let mut fixture = prepare(remote, &server, &[]).await;
        let (_, page) = finish(&mut fixture).await;
        let details = tool_details(&fixture.client, &page).await;
        assert_eq!(details.len(), 1);
        let body = unified(&details[0].1);
        assert_eq!(body.path.as_deref(), Some("created.txt"));
        assert_eq!(hunk_rows(body), ["+One", "+Two", "+Three"], "{}", body.text);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn replaces_frozen_captions() {
    for remote in [false, true] {
        let original = "Original one\nOriginal two\n";
        let server = Server::tools(vec![(
            name("edit_file"),
            json!({
                "path":"note.txt",
                "expected_revision":blake3::hash(original.as_bytes()).to_hex().to_string(),
                "edits":[
                    {"old_text":"one","new_text":"1"},
                    {"old_text":"two","new_text":"2"}
                ]
            }),
        )])
        .await;
        let mut fixture = prepare(remote, &server, &[("note.txt", original)]).await;
        let (turn, _) = finish(&mut fixture).await;
        freeze_caption(&fixture.node, turn, name("edit_file")).await;
        let page = history(&fixture.client, fixture.session.id).await;
        // The collapsed summary keeps the frozen caption; only the detail projection changes.
        assert_eq!(captions(&page), 1);
        let details = tool_details(&fixture.client, &page).await;
        assert_eq!(details.len(), 1);
        let body = unified(&details[0].1);
        assert_eq!(body.path.as_deref(), Some("note.txt"));
        for row in [
            "-Original one\n",
            "+Original 1\n",
            "-Original two\n",
            "+Original 2\n",
        ] {
            assert!(body.text.contains(row), "{}", body.text);
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn keeps_failed_edit_without_diff() {
    for remote in [false, true] {
        let server = Server::tools(vec![(
            name("edit_file"),
            json!({
                "path":"note.txt","old_text":"Original","new_text":"Edited",
                "expected_revision":blake3::hash(b"stale").to_hex().to_string()
            }),
        )])
        .await;
        let mut fixture = prepare(remote, &server, &[("note.txt", "Original")]).await;
        let (turn, page) = finish(&mut fixture).await;
        let details = tool_details(&fixture.client, &page).await;
        assert_eq!(details.len(), 1);
        let detail = &details[0].1;
        assert_eq!(detail.result.as_ref().unwrap()["isError"], true);
        assert!(
            shown(detail)
                .body
                .as_ref()
                .is_none_or(|body| body.diff.is_none()),
            "{detail:?}"
        );
        assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(
            |part| matches!(part, Part::ToolResult { summary, .. } if summary.failure.is_some())
        ));
        assert!(
            checkpoints(&fixture.client, fixture.session.id, turn)
                .await
                .is_empty()
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("note.txt")).unwrap(),
            "Original"
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn keeps_truncated_diff_partial() {
    for remote in [false, true] {
        let text: String = (0..5000)
            .map(|number| format!("Line {number:05}\n"))
            .collect();
        let server = Server::tools(vec![(
            name("write_file"),
            json!({"path":"large.txt","text":text,"expected_revision":null}),
        )])
        .await;
        let mut fixture = prepare(remote, &server, &[]).await;
        let (_, page) = finish(&mut fixture).await;
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("large.txt")).unwrap(),
            text
        );
        let details = tool_details(&fixture.client, &page).await;
        assert_eq!(details.len(), 1);
        let body = unified(&details[0].1);
        assert_eq!(body.path.as_deref(), Some("large.txt"));
        assert!(body.text.len() <= sailry_protocol::MAX_DIFF_BYTES);
        assert!(body.text.contains("+Line 00000\n"), "{}", body.text.len());
        assert!(!body.text.contains("Line 04999"));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

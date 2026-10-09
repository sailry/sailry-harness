use super::*;
use sailry_protocol::hooks::{Script, Settings};

#[tokio::test]
async fn skips_scripts_in_read_only_children() {
    for remote in [false, true] {
        let parent = Server::parallel(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"explore","task":"Read source.txt"}),
        )])
        .await;
        let child = Server::tools(vec![(
            plugin_tool("files", "read_file"),
            json!({"path":"source.txt"}),
        )])
        .await;
        let mut fixture = Fixture::start(remote, parent, child).await;
        std::fs::write(fixture.root().join("source.txt"), "read-only fixture").unwrap();
        select(&mut fixture, true).await;
        let script = Some(Script {
            command: "printf '%s\\n' \"$SAILRY_HOOK_TURN\" >> script-turns.txt".into(),
            timeout_ms: 5_000,
        });
        execute(
            &fixture.client,
            Command::SaveHooks {
                worktree: fixture.session.worktree,
                expected_revision: 0,
                settings: Settings {
                    enabled: true,
                    before_tool: script.clone(),
                    after_tool: script.clone(),
                    turn_end: script,
                    ..Default::default()
                },
            },
        )
        .await;
        let turn = fixture.push("Delegate the exploration").await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Completed,
            "{page:?}"
        );
        let snapshot = fixture.snapshot().await;
        let child = snapshot
            .sessions
            .iter()
            .find(|session| session.delegation.is_some())
            .unwrap();
        assert_eq!(child.config.mode, WorkMode::Plan);
        let child_page = history(&fixture.client, child.id).await;
        assert!(child_page.entries.iter().flat_map(|entry|&entry.parts).any(|part|matches!(part,Part::ToolResult {name,..} if name==&plugin_tool("files","read_file"))));
        let script_turns =
            std::fs::read_to_string(fixture.root().join("script-turns.txt")).unwrap();
        assert!(script_turns.lines().count() >= 3);
        assert!(script_turns.lines().all(|line| line == turn.id.to_string()));
        fixture.close().await;
    }
}

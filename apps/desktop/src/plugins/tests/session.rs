use super::*;
use sailry_protocol::{
    Output,
    conversation::{Part, Status},
};
use serde_json::json;

pub(super) fn mcp(fixture: &Fixture) {
    let root = fixture.directory.path().join("project/package");
    std::fs::write(
        root.join("mcp.json"),
        json!({
            "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
            "mcpServers": {"report": crate::mcp_peer::config("mcp_peer::stdio_peer", "normal")}
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(root.join("version.txt"), "project-summary").unwrap();
}

pub(super) fn run(
    fixture: &mut Fixture,
    package: &sailry_protocol::plugin::Info,
) -> crate::agent_fixture::Server {
    let server = fixture
        .runtime
        .block_on(crate::agent_fixture::Server::tools(vec![
            (
                "load_skill".into(),
                json!({"skill": "project-summary:project-summary"}),
            ),
            (
                crate::mcp_peer::package_alias("project-summary", "report", "read"),
                json!({"value": "report requested"}),
            ),
        ]));
    let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
        panic!("snapshot expected")
    };
    // This fixture exercises only the installed skill and MCP package.
    for other in snapshot
        .plugins
        .iter()
        .filter(|entry| entry.enabled && entry.name != package.summary.name)
    {
        let Output::Plugin(disabled) = fixture.execute(Command::SetPluginEnabled {
            name: other.name.clone(),
            expected_revision: other.revision,
            enabled: false,
        }) else {
            panic!("plugin expected")
        };
        assert!(!disabled.summary.enabled);
    }
    let mut provider = snapshot.providers[0].clone();
    provider.endpoint = server.endpoint.clone();
    provider.models[0].context = 128000;
    fixture.execute(Command::PutProvider {
        expected_revision: provider.revision,
        provider,
    });
    let Output::QueuedTurn(turn) = fixture.execute(Command::SubmitTurn {
        session: fixture.session.id,
        expected_revision: fixture.session.revision,
        message: "Load the project summary skill".into(),
    }) else {
        panic!("turn expected")
    };
    assert_eq!(turn.plugins, vec![package.summary.reference()]);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let Output::Conversation(page) = fixture.execute(Command::ReadConversation {
            session: fixture.session.id,
            before: None,
            limit: 100,
        }) else {
            panic!("conversation expected")
        };
        let page = page.page;
        if page.runs.iter().any(|run| {
            run.turn == turn.id
                && matches!(
                    run.status,
                    Status::Completed | Status::Cancelled | Status::Interrupted | Status::Failed
                )
        }) {
            assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
            let results: Vec<_> = page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter_map(|part| {
                    if let Part::ToolResult { result, .. } = part {
                        Some(result)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(results.len(), 2);
            assert_eq!(
                results[0]["content"],
                include_str!(
                    "../../../../../examples/plugins/project-summary/skills/project-summary/SKILL.md"
                )
            );
            assert_eq!(results[1]["output"]["version"], "project-summary");
            assert_eq!(results[1]["output"]["value"], "report requested");
            return server;
        }
        assert!(Instant::now() < deadline, "plugin skill deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn disabled(fixture: &Fixture) {
    // Ordinary sessions capture the enabled inventory at each admission.
    // Disabling a package removes it from new work, not the session itself.
    let Output::QueuedTurn(turn) = fixture.execute(Command::QueueTurn {
        session: fixture.session.id,
        expected_revision: fixture.session.revision,
        message: "Continue without the disabled plugin".into(),
    }) else {
        panic!("queued turn expected")
    };
    assert!(turn.plugins.is_empty());
    let Output::QueuedMessage(saved) = fixture.execute(Command::ReadQueuedTurn { turn: turn.id })
    else {
        panic!("queued message expected")
    };
    assert_eq!(saved.turn, turn);
}

use super::mcp_peer;
use super::{configuration::support, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{
    Command, Output,
    conversation::{Status, question::State},
};
use serde_json::json;
use std::sync::atomic::Ordering;
#[path = "../../../node-runtime/tests/support/mcp_oauth.rs"]
#[allow(dead_code)]
mod issuer;
#[path = "../../../node-runtime/tests/support/mcp_input.rs"]
#[allow(dead_code, unused_imports)]
mod peer;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn resumes_host_forms() {
    forms(false, false, Input::Form);
}

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn resumes_host_sse_forms() {
    forms(true, false, Input::Form);
}

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn authorizes_host_mcp_tools() {
    for sse in [false, true] {
        forms(sse, true, Input::Form);
    }
}

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn resumes_host_url_requests() {
    for sse in [false, true] {
        forms(sse, false, Input::Url);
    }
}

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn resumes_host_tasks() {
    for sse in [false, true] {
        forms(sse, false, Input::Task);
    }
}

enum Input {
    Form,
    Url,
    Task,
}

fn forms(sse: bool, oauth: bool, input: Input) {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let url = !matches!(input, Input::Form);
    let peer = runtime.block_on(async {
        match input {
            Input::Url => {
                peer::Server::url("https://example.invalid/continue?task=fixture", sse).await
            }
            Input::Task => {
                peer::Server::task(
                    json!({"mode":"url","url":"https://example.invalid/continue?task=fixture"}),
                    sse,
                )
                .await
            }
            Input::Form => peer::Server::with_transport(peer::form_schema(), sse).await,
        }
    });
    peer.package(&directory.path().join("project/package"));
    let issuer = oauth.then(|| runtime.block_on(issuer::Server::start(&peer.endpoint)));
    if let Some(issuer) = &issuer {
        std::fs::write(directory.path().join("project/package/mcp.json"), json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
            "mcpServers":{"input":{"type":if sse {"sse"} else {"streamable-http"},"url":issuer.endpoint}}
        }).to_string()).unwrap();
    }
    let model = runtime.block_on(support::Server::turn_tools(vec![
        (
            peer::alias("input", "read"),
            json!({})
        );
        2
    ]));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &model.endpoint,
        "ffi-mcp-input",
    ));
    runtime.block_on(async {
        let client = Client::new(node.local());
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let Output::Plugin(_plugin) = client
            .execute(client.prepare(Command::InstallPlugin {
                worktree: snapshot.sessions[0].worktree,
                path: "package".into(),
                name: "example".into(),
                expected_revision: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("plugin expected")
        };
    });
    let invitation = node.link().invite().unwrap();
    let child = super::command("mcp_input.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSIONS", serde_json::to_string(&sessions).unwrap())
        .env("SAILRY_MCP_OAUTH", if oauth { "1" } else { "0" })
        .env("SAILRY_MCP_URL", if url { "1" } else { "0" })
        .spawn()
        .unwrap();
    let status = super::wait(child);
    let pages = runtime.block_on(tools::pages(&Client::new(node.local()), &sessions));
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    assert_eq!(pages[0].runs[0].status, Status::Completed);
    assert_eq!(pages[0].questions[1].state, State::Declined);
    assert_eq!(pages[0].runs[1].status, Status::Completed);
    assert_eq!(pages[0].questions[2].turn, pages[0].runs[1].turn);
    assert_eq!(pages[0].questions[3].state, State::Declined);
    assert_eq!(pages[1].runs[0].status, Status::Cancelled);
    assert!(matches!(
        pages[1].questions[0].state,
        State::Cancelled | State::Interrupted
    ));
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    assert_eq!(
        runtime.block_on(tools::pages(&Client::new(node.local()), &sessions)),
        pages
    );
    runtime.block_on(node.shutdown()).unwrap();
    assert_eq!(peer.calls.load(Ordering::SeqCst), 5);
    assert_eq!(peer.initializations.load(Ordering::SeqCst), 2);
    if let Some(issuer) = issuer {
        assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 1);
    }
    assert_eq!(model.requests.lock().unwrap().len(), 7);
}

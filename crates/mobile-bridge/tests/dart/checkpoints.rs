use super::{configuration::support, tools};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Output, RequestId, conversation::checkpoint};
use serde_json::json;

#[cfg(target_os = "macos")]
#[path = "../../../node-runtime/tests/support/trash.rs"]
mod trash;

#[test]
#[ignore = "requires built bridge and Dart; macOS recovers a unique fixture from Trash"]
fn recovers_restore_receipts() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let original = "Original source 中文 🙂";
    let middle = "Middle source 中文 🙂";
    let latest = "Final source 中文 🙂";
    let before = "完整旧文件 中文 🙂\n".repeat(4000);
    let after = "完整新文件 中文 🙂\n".repeat(2000);
    let created = format!("sailry-ffi-checkpoint-{}-资料.txt", RequestId::new());
    std::fs::write(root.join("source.txt"), original).unwrap();
    std::fs::write(root.join("large.txt"), &before).unwrap();
    #[cfg(target_os = "macos")]
    let recycled = trash::Entry::new(&root, created.clone());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let calls = [
        ("source.txt", Some(original), middle),
        ("source.txt", Some(middle), latest),
        ("large.txt", Some(before.as_str()), after.as_str()),
        (created.as_str(), None, "Created file 中文 🙂"),
    ].into_iter().map(|(path, before, text)| (
        super::agent_support::plugin_tool("files", "write_file"),
        json!({"path": path, "text": text, "expected_revision": before.map(|text| blake3::hash(text.as_bytes()).to_hex().to_string())}),
    )).collect();
    let server = runtime.block_on(support::Server::tools(calls));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-checkpoints",
    ));
    let client = Client::new(node.local());
    let invitation = node.link().invite().unwrap();
    let child = super::command("checkpoints.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSIONS", serde_json::to_string(&sessions).unwrap())
        .env("SAILRY_CREATED_FILE", &created)
        .env(
            "SAILRY_NATIVE_TRASH",
            if cfg!(target_os = "macos") { "1" } else { "0" },
        )
        .spawn()
        .unwrap();
    let status = super::wait(child);
    let Output::Snapshot(snapshot) = runtime
        .block_on(client.execute(client.prepare(Command::Snapshot)))
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    assert!(status.success());
    assert_eq!(snapshot.sessions.len(), 4);
    let ids: Vec<_> = snapshot.sessions.iter().map(|session| session.id).collect();
    let pages = runtime.block_on(tools::pages(&client, &ids));
    let source = ids.iter().position(|id| *id == sessions[0]).unwrap();
    assert_eq!(pages[source].revision, 2);
    assert!(pages[source].entries.is_empty());
    let branches: Vec<_> = snapshot
        .sessions
        .iter()
        .filter(|session| session.fork.is_some())
        .collect();
    let mut saved = Vec::new();
    for branch in &branches {
        let page = &pages[ids.iter().position(|id| *id == branch.id).unwrap()];
        assert_eq!(page.runs.len(), 1);
        super::paging::assert_history(page, 1, 1, 4, "ffi-checkpoints");
        let Output::FileCheckpoints(files) = runtime
            .block_on(client.execute(client.prepare(Command::ListFileCheckpoints {
                session: branch.id,
                turn: page.runs[0].turn,
                before: None,
                limit: 100,
            })))
            .unwrap()
        else {
            panic!("checkpoints expected")
        };
        assert_eq!(files.files.len(), 4);
        saved.push(files);
    }
    assert_eq!(saved[0].files, saved[1].files);
    runtime.block_on(node.shutdown()).unwrap();
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    let client = Client::new(node.local());
    assert_eq!(runtime.block_on(tools::pages(&client, &ids)), pages);
    for page in saved {
        let Output::FileCheckpoints(restored) = runtime
            .block_on(client.execute(client.prepare(Command::ListFileCheckpoints {
                session: page.session,
                turn: page.turn,
                before: None,
                limit: 100,
            })))
            .unwrap()
        else {
            panic!("checkpoints expected")
        };
        assert_eq!(restored, page);
        let file: &checkpoint::File = page
            .files
            .iter()
            .find(|file| file.path == "large.txt")
            .unwrap();
        let Output::FileCheckpoint(content) = runtime
            .block_on(client.execute(client.prepare(Command::ReadFileCheckpoint {
                session: page.session,
                checkpoint: file.id,
            })))
            .unwrap()
        else {
            panic!("checkpoint content expected")
        };
        assert_eq!(content.before.as_deref(), Some(before.as_str()));
        assert_eq!(content.after, after);
    }
    runtime.block_on(node.shutdown()).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("source.txt")).unwrap(),
        original
    );
    assert_eq!(
        std::fs::read_to_string(root.join("large.txt")).unwrap(),
        before
    );
    #[cfg(target_os = "macos")]
    {
        assert_eq!(recycled.name, created);
        assert_eq!(
            std::fs::read_to_string(&recycled.source).unwrap(),
            "Replacement survives retry"
        );
        assert_eq!(
            std::fs::read_to_string(recycled.recover()).unwrap(),
            "Created file 中文 🙂"
        );
    }
    #[cfg(not(target_os = "macos"))]
    assert_eq!(
        std::fs::read_to_string(root.join(created)).unwrap(),
        "Created file 中文 🙂"
    );
    assert_eq!(server.requests.lock().unwrap().len(), 5);
}

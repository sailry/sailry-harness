use sailry_client::Client;
use sailry_protocol::*;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn observes_desktop_order() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let node = runtime
        .block_on(sailry_node_runtime::Node::start(
            directory.path().join("node"),
        ))
        .unwrap();
    let client = Client::new(node.local());
    let Output::Project(project) = runtime
        .block_on(client.execute(client.prepare(Command::RegisterProject {
            name: "Session order".into(),
            path: directory.path().to_str().unwrap().into(),
        })))
        .unwrap()
    else {
        panic!("project expected")
    };
    let mut sessions = Vec::new();
    for _ in 0..3 {
        let Output::Session(session) = runtime
            .block_on(client.execute(client.prepare(Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: ProviderId::new(),
                    model: "fixture".into(),
                    effort: Effort::Default,
                    mode: WorkMode::Code,
                    permission: Permission::Ask,
                    credential: None,
                }),
            })))
            .unwrap()
        else {
            panic!("session expected")
        };
        sessions.push(session.id);
    }
    let ready = directory.path().join("mobile-ready");
    let invitation = node.link().invite().unwrap();
    let mut child = super::command("session_order.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSIONS", serde_json::to_string(&sessions).unwrap())
        .env("SAILRY_READY", &ready)
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !ready.exists() {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("Dart observer exited before ready: {status}");
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("Dart observer readiness deadline");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    runtime
        .block_on(client.execute(client.prepare(Command::SetSessionOrder {
            project: project.id,
            expected: sessions.iter().rev().copied().collect(),
            sessions: vec![sessions[0], sessions[2], sessions[1]],
        })))
        .unwrap();
    let status = super::wait(child);
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
}

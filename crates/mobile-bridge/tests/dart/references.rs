use super::{configuration::support, tools};
use sailry_client::Client;
use sailry_protocol::*;

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn delegates_and_restores_context() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("source.txt"), "Reference contents").unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(support::Server::start(false));
    let (node, sessions) = runtime.block_on(tools::sessions(
        directory.path(),
        &server.endpoint,
        "ffi-reference",
    ));
    let role = runtime.block_on(async {
        let client = Client::new(node.local());
        let Output::Role(role) = client
            .execute(client.prepare(Command::PutRole {
                expected_revision: 0,
                role: role::Profile {
                    appearance: None,
                    id: RoleId::new(),
                    revision: 0,
                    key: "review".into(),
                    name: "Reviewer".into(),
                    description: "Review the selected context".into(),
                    model: None,
                    max_turns: Some(5),
                    skills: vec![],
                    instructions: "Inspect the supplied references".into(),
                },
            }))
            .await
            .unwrap()
        else {
            panic!("role expected");
        };
        client
            .execute(client.prepare(Command::SetSessionRoles {
                session: sessions[0],
                expected_revision: 1,
                roles: vec![role.reference()],
            }))
            .await
            .unwrap();
        role.reference()
    });
    let invitation = node.link().invite().unwrap();
    let process = super::command("references.dart", directory.path(), invitation.ticket())
        .env("SAILRY_SESSION", sessions[0].to_string())
        .env("SAILRY_ROLE", serde_json::to_string(&role).unwrap())
        .spawn()
        .unwrap();
    let status = super::wait(process);
    runtime.block_on(node.shutdown()).unwrap();
    assert!(status.success());
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Output, RoleId};

#[test]
#[ignore = "requires built bridge and dart pub get in tests/mobile-contract"]
fn persists_catalog_without_controller() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let profile = directory.path().join("node");
    let node = runtime.block_on(Node::start(&profile)).unwrap();
    let client = Client::new(node.local());
    let mut provider = runtime.block_on(super::configuration::provider(
        &client,
        "http://127.0.0.1:9/v1",
        "ffi-role",
    ));
    provider.models[0]
        .capabilities
        .get_or_insert_with(Default::default)
        .reasoning = Some(false);
    let Output::Provider(provider) = runtime
        .block_on(client.execute(client.prepare(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        })))
        .unwrap()
    else {
        panic!("provider expected")
    };
    let original = RoleId::new();
    let replacement = RoleId::new();
    let invitation = node.link().invite().unwrap();
    let child = super::command("roles.dart", directory.path(), invitation.ticket())
        .env("SAILRY_ROLE", original.to_string())
        .env("SAILRY_REPLACEMENT_ROLE", replacement.to_string())
        .env("SAILRY_ROLE_PROVIDER", provider.id.to_string())
        .spawn()
        .unwrap();
    let status = super::wait(child);
    let expected = runtime.block_on(async {
        let roles = client
            .execute(client.prepare(Command::ListRoles))
            .await
            .unwrap();
        node.shutdown().await.unwrap();
        roles
    });
    assert!(status.success());
    let Output::Roles(roles) = &expected else {
        panic!("role catalog expected")
    };
    assert_eq!(roles.len(), 1);
    assert_eq!(roles[0].id, replacement);
    assert_eq!(roles[0].revision, 1);
    assert_eq!(roles[0].key, "review");
    assert_eq!(roles[0].model.as_ref().unwrap().provider, provider.id);
    assert_eq!(roles[0].skills, ["local-review"]);
    runtime.block_on(async {
        let node = Node::start(&profile).await.unwrap();
        let client = Client::new(node.local());
        assert_eq!(
            client
                .execute(client.prepare(Command::ListRoles))
                .await
                .unwrap(),
            expected
        );
        node.shutdown().await.unwrap();
        let other = Node::start(directory.path().join("other")).await.unwrap();
        let client = Client::new(other.local());
        let Output::Roles(roles) = client
            .execute(client.prepare(Command::ListRoles))
            .await
            .unwrap()
        else {
            panic!("role catalog expected")
        };
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].key, "explore");
        assert!(roles[0].read_only);
        other.shutdown().await.unwrap();
    });
}

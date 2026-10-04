use sailry_client::{Apply, Client, Projection};
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{conversation::*, *};
use std::time::Duration;
#[path = "roles/sessions.rs"]
mod sessions;

fn profile(key: &str) -> role::Profile {
    role::Profile {
        appearance: None,
        id: RoleId::new(),
        revision: 0,
        key: key.into(),
        name: "Review 中文 🙂".into(),
        description: "Review the delegated implementation".into(),
        model: None,
        max_turns: Some(16),
        skills: vec![],
        instructions: "Report concrete findings".into(),
    }
}

async fn list(client: &Client) -> Vec<role::Profile> {
    let Output::Roles(roles) = client
        .execute(client.prepare(Command::ListRoles))
        .await
        .unwrap()
    else {
        panic!("roles expected")
    };
    roles
}

async fn put(
    client: &Client,
    role: role::Profile,
    expected_revision: u64,
) -> Result<role::Profile, Fault> {
    match client
        .execute(client.prepare(Command::PutRole {
            role,
            expected_revision,
        }))
        .await?
    {
        Output::Role(role) => Ok(role),
        _ => panic!("role expected"),
    }
}

async fn fixture(remote: bool) -> (tempfile::TempDir, Node, Link, Client) {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start(directory.path().join("node")).await.unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let client = Client::new(if remote {
        controller.handle().remote(address)
    } else {
        node.local()
    });
    (directory, node, controller, client)
}

#[tokio::test]
async fn recovers_versioned_receipts() {
    for remote in [false, true] {
        let (directory, node, controller, client) = fixture(remote).await;
        let mut updates = client.subscribe().await.unwrap();
        let mut projection = Projection::new(node.id(), 1);
        assert_eq!(
            projection.apply(1, updates.next().await.unwrap()).unwrap(),
            Apply::Applied
        );
        assert!(list(&client).await.is_empty());
        let request = client.prepare(Command::PutRole {
            role: profile("review"),
            expected_revision: 0,
        });
        let admitted = client.dispatch(request.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        drop(admitted);
        let saved = client.execute(request.clone()).await.unwrap();
        let Output::Role(first) = &saved else {
            panic!("role expected")
        };
        assert_eq!(first.revision, 1);
        let mut edited = first.clone();
        edited.key = "analysis".into();
        edited.description.clear();
        edited.appearance = Some(projects::Appearance {
            icon: "code".into(),
            color: "violet".into(),
        });
        edited.instructions = "Analyze the delegated task 中文 🙂".into();
        let edited = put(&client, edited, 1).await.unwrap();
        assert_eq!(edited.id, first.id);
        assert_eq!(edited.revision, 2);
        assert_eq!(
            put(&client, first.clone(), 1).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            put(&client, profile("analysis"), 0).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        assert_eq!(client.execute(request.clone()).await.unwrap(), saved);
        assert_eq!(
            list(&client).await.as_slice(),
            std::slice::from_ref(&edited)
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            while projection.snapshot().unwrap().roles.as_slice() != std::slice::from_ref(&edited) {
                assert_eq!(
                    projection.apply(1, updates.next().await.unwrap()).unwrap(),
                    Apply::Applied
                );
            }
        })
        .await
        .unwrap();
        let read = client
            .dispatch(client.prepare(Command::ListRoles))
            .await
            .unwrap();
        assert!(!read.receipt.durable);
        assert_eq!(
            read.completion.await.unwrap().unwrap(),
            Output::Roles(vec![edited.clone()])
        );
        drop(updates);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(client.execute(request).await.unwrap(), saved);
        assert_eq!(
            list(&client).await.as_slice(),
            std::slice::from_ref(&edited)
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::RemoveRole {
                    role: edited.id,
                    expected_revision: 1
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let remove = client.prepare(Command::RemoveRole {
            role: edited.id,
            expected_revision: 2,
        });
        assert_eq!(
            client.execute(remove.clone()).await.unwrap(),
            Output::Roles(vec![])
        );
        let replacement = put(&client, profile("analysis"), 0).await.unwrap();
        assert_eq!(client.execute(remove).await.unwrap(), Output::Roles(vec![]));
        assert_eq!(list(&client).await, [replacement]);
        let other = Node::start(directory.path().join("other")).await.unwrap();
        assert!(list(&Client::new(other.local())).await.is_empty());
        other.shutdown().await.unwrap();
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn validates_fixed_models() {
    for remote in [false, true] {
        let (_directory, node, controller, client) = fixture(remote).await;
        let mut role = profile("fixed");
        let mut provider = Provider {
            options: None,
            id: ProviderId::new(),
            revision: 0,
            name: "Unused fixture endpoint".into(),
            api: ModelApi::ChatCompletions,
            authentication: sailry_protocol::Authentication::ApiKey,
            endpoint: "http://127.0.0.1:9/v1".into(),
            enabled: true,
            default_model: "fixture".into(),
            credential: None,
            models: vec![Model {
                id: "fixture".into(),
                context: 128,
                output: 32,
                vision: false,
                tools: true,
                reasoning: true,
                web_search: false,
                generates: vec![],
                efforts: vec![sailry_protocol::Effort::Low, sailry_protocol::Effort::High],
                custom_efforts: false,
                default_effort: sailry_protocol::Effort::High,
            }],
        };
        role.model = Some(role::Model {
            provider: provider.id,
            model: "fixture".into(),
            effort: Some(Effort::High),
        });
        assert_eq!(
            put(&client, role.clone(), 0).await.unwrap_err().code,
            ErrorCode::NotConfigured
        );
        let Output::Provider(saved) = client
            .execute(client.prepare(Command::PutProvider {
                provider: provider.clone(),
                expected_revision: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        provider = saved;
        let mut unavailable = role.clone();
        unavailable.model.as_mut().unwrap().model = "missing".into();
        assert_eq!(
            put(&client, unavailable, 0).await.unwrap_err().code,
            ErrorCode::NotConfigured
        );
        let mut invalid = role.clone();
        invalid.model.as_mut().unwrap().effort = Some(Effort::Max);
        assert_eq!(
            put(&client, invalid, 0).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        let saved = put(&client, role, 0).await.unwrap();
        provider.enabled = false;
        client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        assert_eq!(
            put(&client, saved.clone(), 1).await.unwrap_err().code,
            ErrorCode::NotConfigured
        );
        assert_eq!(list(&client).await, [saved]);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn bounds_new_entries() {
    for remote in [false, true] {
        let (_directory, node, controller, client) = fixture(remote).await;
        let mut first = None;
        for index in 0..role::MAX_PROFILES {
            let saved = put(&client, profile(&format!("role-{index:02}")), 0)
                .await
                .unwrap();
            if index == 0 {
                first = Some(saved);
            }
        }
        assert_eq!(
            put(&client, profile("overflow"), 0).await.unwrap_err().code,
            ErrorCode::Busy
        );
        let mut edited = first.unwrap();
        edited.name = "Edited at capacity".into();
        put(&client, edited, 1).await.unwrap();
        let saved = list(&client).await;
        assert_eq!(saved.len(), role::MAX_PROFILES);
        assert!(saved.windows(2).all(|roles| roles[0].key < roles[1].key));
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

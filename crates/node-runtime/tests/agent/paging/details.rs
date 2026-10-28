use super::*;
use sailry_protocol::tool::detail::{Detail, MAX_DETAIL_PAGE_BYTES, Reference};

const PACKAGE: &str = "detail-fixture";

fn command(reference: &Reference, cursor: Option<usize>, limit: usize) -> Command {
    Command::ReadToolDetail {
        session: reference.session,
        turn: reference.turn,
        entry: reference.entry.clone(),
        index: reference.index,
        call: reference.call.clone(),
        expected_revision: reference.revision.clone(),
        cursor,
        limit,
    }
}

async fn install(client: &Client, project: &std::path::Path, worktree: WorktreeId) -> plugin::Info {
    let root = project.join("package");
    std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/host/main.js"),
        "export function inspect(args) { return {text:args.text,complete:true}; }",
    )
    .unwrap();
    std::fs::write(
        root.join("plugin.json"),
        json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name":PACKAGE,"version":"0.1.0",
            "extensions":{"dev.sailry.platform":{
                "api_version":"v1","actions":[],
                "host":{"entry":"dev.sailry.platform/host/main.js",
                    "resources":["dev.sailry.platform/host/main.js"],"handlers":["inspect"]},
                "tools":[{"name":"inspect","description":"Return canonical Unicode content",
                    "presentation":"details",
                    "display":{"label":"Inspect","output":{"body":{"text":{"source":"result","path":"/text"}}}},
                    "handler":{"name":"inspect","parameters":{"type":"object","required":["text"],
                        "properties":{"text":{"type":"string"}},"additionalProperties":false}}}]
            }}
        })
        .to_string(),
    )
    .unwrap();
    let Output::Plugin(package) = client
        .execute(client.prepare(Command::InstallPlugin {
            worktree,
            path: "package".into(),
            name: PACKAGE.into(),
            expected_revision: 0,
            expected_digest: None,
        }))
        .await
        .unwrap()
    else {
        panic!("detail fixture package expected")
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    package
}

#[tokio::test]
async fn pages_canonical_details_on_both_transports() {
    for remote in [false, true] {
        let text = "Canonical Unicode 中文🙂\n".repeat(8000);
        let arguments = json!({"text":text});
        let model = Server::tools(vec![(plugin_tool(PACKAGE, "inspect"), arguments.clone())]).await;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
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
        let session = approvals::prepare_packages(&client, &model, &root, &[]).await;
        let package = install(&client, &root, session.worktree).await;
        let Output::Providers(mut providers) = client
            .execute(client.prepare(Command::ListProviders))
            .await
            .unwrap()
        else {
            panic!("providers expected")
        };
        let mut provider = providers.remove(0);
        provider.models[0].context = 2_000_000;
        let revision = provider.revision;
        client
            .execute(client.prepare(Command::PutProvider {
                provider,
                expected_revision: revision,
            }))
            .await
            .unwrap();
        let turn = approvals::submit(&client, session.id).await;
        let page = finished(&client, session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert!(serde_json::to_vec(&page).unwrap().len() < 16 * 1024);
        assert!(!serde_json::to_string(&page).unwrap().contains(&text));
        let (entry, index, call, summary) = page
            .entries
            .iter()
            .find_map(|entry| {
                entry
                    .parts
                    .iter()
                    .enumerate()
                    .find_map(|(index, part)| match part {
                        Part::ToolResult { id, summary, .. } => Some((entry, index, id, summary)),
                        _ => None,
                    })
            })
            .unwrap();
        assert!(summary.has_details);
        assert!(!summary.raw_result);
        let reference = Reference {
            session: page.session,
            turn: entry.turn,
            entry: entry.id.clone(),
            index,
            call: call.clone(),
            revision: summary.revision.clone(),
        };
        let mut restored = String::new();
        loop {
            let offset = restored.len();
            let Output::ToolDetail(part) = client
                .execute(client.prepare(command(&reference, (offset > 0).then_some(offset), 4093)))
                .await
                .unwrap()
            else {
                panic!("canonical detail page expected")
            };
            assert_eq!(part.reference, reference);
            assert_eq!(part.offset, offset);
            assert!(part.text.len() <= 4093);
            assert!(serde_json::to_vec(&part).unwrap().len() < 4093 + 4096);
            restored.push_str(&part.text);
            if let Some(next) = part.next_offset {
                assert_eq!(next, restored.len());
                assert!(next < part.total_bytes);
            } else {
                assert_eq!(restored.len(), part.total_bytes);
                break;
            }
        }
        assert!(restored.len() > MAX_DETAIL_PAGE_BYTES);
        let detail: Detail = serde_json::from_str(&restored).unwrap();
        assert_eq!(detail.arguments, arguments);
        assert_eq!(detail.result, Some(json!({"text":text,"complete":true})));
        let unicode = restored
            .char_indices()
            .find(|(_, character)| !character.is_ascii())
            .unwrap()
            .0;
        for (cursor, limit) in [
            (Some(unicode + 1), 4093),
            (Some(restored.len() + 1), 4093),
            (None, 0),
            (None, MAX_DETAIL_PAGE_BYTES + 1),
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command(&reference, cursor, limit)))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        for (invalid, code) in [
            (
                Reference {
                    revision: "0".repeat(64),
                    ..reference.clone()
                },
                ErrorCode::RevisionConflict,
            ),
            (
                Reference {
                    call: Some("missing".into()),
                    ..reference.clone()
                },
                ErrorCode::NotFound,
            ),
            (
                Reference {
                    call: None,
                    ..reference.clone()
                },
                ErrorCode::NotFound,
            ),
            (
                Reference {
                    entry: "missing".into(),
                    ..reference.clone()
                },
                ErrorCode::NotFound,
            ),
            (
                Reference {
                    index: usize::MAX,
                    ..reference.clone()
                },
                ErrorCode::NotFound,
            ),
            (
                Reference {
                    session: SessionId::new(),
                    ..reference.clone()
                },
                ErrorCode::WrongTarget,
            ),
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command(&invalid, None, 100)))
                    .await
                    .unwrap_err()
                    .code,
                code
            );
        }
        let denied = client
            .prepare(command(&reference, None, 100))
            .with_plugin(plugin::Context {
                invocation: None,
                turn: None,
                surface: Default::default(),
                package: package.summary.reference(),
                worktree: Some(session.worktree),
                session: Some(session.id),
            });
        assert_eq!(
            client.execute(denied).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        let cached = crate::agent_support::details::read(&client, reference.clone())
            .await
            .unwrap();
        assert_eq!(cached.as_ref(), &detail);
        assert!(Arc::ptr_eq(
            &cached,
            &crate::agent_support::details::read(&client, reference.clone())
                .await
                .unwrap()
        ));

        let profile = node.profile().to_owned();
        node.shutdown().await.unwrap();
        let node = Node::start_empty(profile).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            crate::agent_support::details::read(&client, reference.clone())
                .await
                .unwrap()
                .as_ref(),
            &detail
        );
        let Output::Session(branch) = client
            .execute(client.prepare(Command::ForkConversation {
                session: session.id,
                through: turn,
                expected_revision: page.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("forked session expected")
        };
        let inherited = Reference {
            session: branch.id,
            ..reference.clone()
        };
        assert_eq!(
            crate::agent_support::details::read(&client, inherited)
                .await
                .unwrap()
                .as_ref(),
            &detail
        );
        client
            .execute(client.prepare(Command::RewindConversation {
                session: session.id,
                through: None,
                expected_head: turn,
                expected_revision: page.revision,
            }))
            .await
            .unwrap();
        assert_eq!(
            client
                .execute(client.prepare(command(&reference, None, 100)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::WrongTarget
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

use super::*;
use process::Fixture;
use serde_json::json;
#[path = "../memory_support/mod.rs"]
mod support;
use support::{Entry, Kind, MemoryId, Summary};

async fn install(fixture: &Fixture) {
    support::prepare(&fixture.root.join("memory"));
    support::install(&fixture.client, fixture.session.worktree, "memory").await;
}

#[tokio::test]
async fn preserves_reads_in_plan() {
    for remote in [false, true] {
        let id = MemoryId::new();
        let calls = vec![
            (
                plugin_tool("memory", "search_memory"),
                json!({"query":id.to_string()}),
            ),
            (plugin_tool("memory", "review_memories"), json!({})),
        ];
        let server = Server::tools(calls.clone()).await;
        let mut fixture = Fixture::new(remote, &server).await;
        install(&fixture).await;
        support::save_settings(
            &fixture.client,
            support::Settings {
                auto_write: true,
                ..support::settings(&fixture.client).await.unwrap()
            },
        )
        .await
        .unwrap();
        support::put(
            &fixture.client,
            Entry {
                summary: Summary {
                    id,
                    project: fixture.session.project,
                    title: "Planning constraint".into(),
                    kind: Kind::Project,
                    revision: 0,
                    updated_at_ms: 0,
                    archived: false,
                },
                body: "Preserve the verified project boundary".into(),
            },
            0,
        )
        .await
        .unwrap();
        let original = support::read(&fixture.client, id).await.unwrap();
        planning::configure(&mut fixture, WorkMode::Plan).await;
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Inspect the project constraints".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        super::evaluation::trajectory(&page, &calls);
        assert_eq!(page.runs[0].status, Status::Completed, "{page:?}");
        assert!(page.approvals.is_empty());
        let results: Vec<_> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::ToolResult { result, .. } => Some(result),
                _ => None,
            })
            .collect();
        assert_eq!(results.len(), 2, "{page:?}");
        let text = results[0]["entries"][0]["parts"][0]["text"]
            .as_str()
            .unwrap();
        let retrieved: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(retrieved["body"], original.body);
        assert_eq!(retrieved["body_truncated"], false);
        assert!(results[1]["candidates"].is_array(), "{}", results[1]);
        assert_eq!(support::read(&fixture.client, id).await.unwrap(), original);
        for request in server.requests.lock().unwrap().iter() {
            let tools = request["tools"].to_string();
            for name in ["search_memory", "review_memories"] {
                assert!(tools.contains(name));
            }
            for name in ["save_memory", "forget_memory", "consolidate_memories"] {
                assert!(!tools.contains(name));
            }
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn curates_scoped_memory() {
    for remote in [false, true] {
        let id = MemoryId::new();
        let foreign_id = MemoryId::new();
        let calls = vec![
            (
                plugin_tool("memory", "save_memory"),
                json!({"expected_revision":0,"kind":"user","title":"Output preference","body":"Use concise responses"}),
            ),
            (
                plugin_tool("memory", "search_memory"),
                json!({"query":"Output preference"}),
            ),
            (
                plugin_tool("memory", "save_memory"),
                json!({"id":id,"expected_revision":1,"kind":"feedback","title":"Existing feedback","body":"Blind overwrite"}),
            ),
            (
                plugin_tool("memory", "search_memory"),
                json!({"query":id.to_string()}),
            ),
            (
                plugin_tool("memory", "save_memory"),
                json!({"id":id,"expected_revision":1,"kind":"feedback","title":"Existing feedback","body":"Use tests before committing"}),
            ),
            (
                plugin_tool("memory", "search_memory"),
                json!({"query":id.to_string()}),
            ),
            (
                plugin_tool("memory", "forget_memory"),
                json!({"id":id,"expected_revision":2}),
            ),
            (
                plugin_tool("memory", "forget_memory"),
                json!({"id":foreign_id,"expected_revision":1}),
            ),
        ];
        let server = Server::tools(calls.clone()).await;
        let mut fixture = Fixture::new(remote, &server).await;
        install(&fixture).await;
        support::save_settings(
            &fixture.client,
            support::Settings {
                auto_write: true,
                ..support::settings(&fixture.client).await.unwrap()
            },
        )
        .await
        .unwrap();
        let mut config = fixture.session.config.clone();
        config.permission = Permission::Full;
        let Output::Session(session) = fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: 1,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected");
        };
        fixture.session = session;
        let other_root = fixture.directory.path().join("other-project");
        std::fs::create_dir(&other_root).unwrap();
        let Output::Project(other) = fixture
            .client
            .execute(fixture.client.prepare(Command::RegisterProject {
                name: "Other".into(),
                path: other_root.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected");
        };
        for (id, project, body) in [
            (id, fixture.session.project, "Original feedback"),
            (
                foreign_id,
                Some(other.id),
                "Do not reveal this other project",
            ),
        ] {
            support::put(
                &fixture.client,
                Entry {
                    summary: Summary {
                        id,
                        project,
                        title: "Existing feedback".into(),
                        kind: Kind::Feedback,
                        revision: 0,
                        updated_at_ms: 0,
                        archived: false,
                    },
                    body: body.into(),
                },
                0,
            )
            .await
            .unwrap();
        }
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: 2,
                message: "Remember my preferences".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected");
        };
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        super::evaluation::trajectory(&page, &calls);
        assert_eq!(
            page.runs[0].status,
            Status::Completed,
            "{:?}",
            page.runs[0].error
        );
        let results: Vec<_> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::ToolResult { result, .. } => Some(result),
                _ => None,
            })
            .collect();
        assert_eq!(results.len(), 8);
        assert_eq!(
            results[0]["updated"][0]["project"],
            json!(fixture.session.project)
        );
        assert!(results[1].to_string().contains("Use concise responses"));
        assert_eq!(results[2]["error"]["code"], "invalid_request");
        assert!(results[3].to_string().contains("Original feedback"));
        assert_eq!(results[4]["updated"][0]["revision"], 2);
        assert!(
            results[5]
                .to_string()
                .contains("Use tests before committing")
        );
        assert_eq!(results[6]["removed"], json!([id]));
        assert_eq!(results[7]["error"]["code"], "permission_denied");
        {
            let requests = server.requests.lock().unwrap();
            let prompt = requests[0]["messages"].to_string();
            assert!(prompt.contains(&id.to_string()));
            assert!(!prompt.contains(&foreign_id.to_string()));
            assert!(!prompt.contains("Original feedback"));
        }
        let entries = support::run::<Vec<Entry>>(
            &fixture.client,
            &support::context(&fixture.client).await,
            json!({"action":"search","project":fixture.session.project,"query":""}),
        )
        .await
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].summary.title, "Output preference");
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn applies_tool_settings() {
    for remote in [false, true] {
        for enabled in [false, true] {
            let calls = if enabled {
                vec![
                    (
                        plugin_tool("memory", "search_memory"),
                        json!({"query":"中文"}),
                    ),
                    (
                        plugin_tool("memory", "search_memory"),
                        json!({"query":"中文"}),
                    ),
                ]
            } else {
                vec![]
            };
            let server = Server::tools(calls).await;
            let fixture = Fixture::new(remote, &server).await;
            install(&fixture).await;
            support::save_settings(
                &fixture.client,
                support::Settings {
                    enabled,
                    auto_write: false,
                    context_bytes: 2048,
                    ..support::settings(&fixture.client).await.unwrap()
                },
            )
            .await
            .unwrap();
            let id = MemoryId::new();
            support::put(
                &fixture.client,
                Entry {
                    summary: Summary {
                        id,
                        project: fixture.session.project,
                        title: "Language preference".into(),
                        kind: Kind::User,
                        revision: 0,
                        updated_at_ms: 0,
                        archived: false,
                    },
                    body: "中文回答🙂\"\\\n".repeat(100),
                },
                0,
            )
            .await
            .unwrap();
            let Output::QueuedTurn(turn) = fixture
                .client
                .execute(fixture.client.prepare(Command::SubmitTurn {
                    session: fixture.session.id,
                    expected_revision: 1,
                    message: "Describe the preference".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                page.runs[0].status,
                Status::Completed,
                "{:?}",
                page.runs[0].error
            );
            {
                let requests = server.requests.lock().unwrap();
                let tools = requests[0]["tools"].to_string();
                assert_eq!(tools.contains("search_memory"), enabled);
                assert!(!tools.contains("save_memory"));
                assert!(!tools.contains("forget_memory"));
                assert!(!tools.contains("consolidate_memories"));
                let prompt = requests[0]["messages"].to_string();
                assert_eq!(prompt.contains(&id.to_string()), enabled);
            }
            if enabled {
                let results: Vec<_> = page
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .filter_map(|part| match part {
                        Part::ToolResult { result, .. } => Some(result),
                        _ => None,
                    })
                    .collect();
                assert_eq!(results.len(), 2);
                assert!(results[0].to_string().contains("body_truncated"));
                assert_eq!(results[1]["entries"].as_array().unwrap().len(), 0);
                assert!(results[0].to_string().len() + results[1].to_string().len() < 2048);
            }
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn consolidates_with_scoped_authority() {
    for remote in [false, true] {
        let target = MemoryId::new();
        let source = MemoryId::new();
        let server = Server::tools(vec![
            (plugin_tool("memory", "review_memories"),json!({})),
            (plugin_tool("memory", "consolidate_memories"),json!({"id":target,"expected_revision":1,"title":"Blind merge","body":"Lost conditions","sources":[{"id":source,"revision":1}]})),
            (plugin_tool("memory", "search_memory"),json!({"query":"Reply preference"})),
            (plugin_tool("memory", "consolidate_memories"),json!({"id":target,"expected_revision":1,"title":"Language preference","body":"Use concise Chinese replies","sources":[{"id":source,"revision":1}]})),
            (plugin_tool("memory", "search_memory"),json!({"query":"Chinese"})),
        ]).await;
        let fixture = Fixture::new(remote, &server).await;
        install(&fixture).await;
        assert_ne!(fixture.session.config.permission, Permission::Full);
        support::save_settings(
            &fixture.client,
            support::Settings {
                auto_write: true,
                ..support::settings(&fixture.client).await.unwrap()
            },
        )
        .await
        .unwrap();
        for (id, body) in [
            (target, "Use Chinese replies"),
            (source, "Use concise replies"),
        ] {
            support::put(
                &fixture.client,
                Entry {
                    summary: Summary {
                        id,
                        project: fixture.session.project,
                        title: "Reply preference".into(),
                        kind: Kind::User,
                        revision: 0,
                        updated_at_ms: 0,
                        archived: false,
                    },
                    body: body.into(),
                },
                0,
            )
            .await
            .unwrap();
        }
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: 1,
                message: "Remember my reply preferences".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs[0].status,
            Status::Completed,
            "{:?}",
            page.runs[0].error
        );
        let results: Vec<_> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::ToolResult { result, .. } => Some(result),
                _ => None,
            })
            .collect();
        assert_eq!(results[1]["error"]["code"], "invalid_request");
        assert!(results[3]["updated"].is_array());
        assert_eq!(page.approvals.len(), 2);
        assert_eq!(page.approvals[0].source, ApprovalSource::Storage);
        let archived = support::read(&fixture.client, source).await.unwrap();
        assert!(archived.summary.archived);
        assert_eq!(archived.body, "Use concise replies");
        let merged = support::read(&fixture.client, target).await.unwrap();
        assert_eq!(merged.body, "Use concise Chinese replies");
        assert_eq!(merged.summary.revision, 2);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_unread_replacement() {
    for remote in [false, true] {
        for parallel in [false, true] {
            let id = MemoryId::new();
            let calls = vec![
                (
                    plugin_tool("memory", "search_memory"),
                    json!({"query":id.to_string()}),
                ),
                (
                    plugin_tool("memory", "save_memory"),
                    json!({"id":id,"expected_revision":1,"kind":"project","title":"Constraint","body":"Lost exceptions"}),
                ),
                (
                    plugin_tool("memory", "save_memory"),
                    json!({"expected_revision":0,"global":true,"kind":"project","title":"Wrong scope","body":"Only this project uses this architecture"}),
                ),
            ];
            let server = if parallel {
                Server::parallel(calls).await
            } else {
                Server::tools(calls).await
            };
            let fixture = Fixture::new(remote, &server).await;
            install(&fixture).await;
            support::save_settings(
                &fixture.client,
                support::Settings {
                    auto_write: true,
                    context_bytes: 2048,
                    ..support::settings(&fixture.client).await.unwrap()
                },
            )
            .await
            .unwrap();
            let original = if parallel {
                "Preserve all exceptions".into()
            } else {
                "保留适用条件和例外🙂".repeat(200)
            };
            support::put(
                &fixture.client,
                Entry {
                    summary: Summary {
                        id,
                        project: fixture.session.project,
                        title: "Constraint".into(),
                        kind: Kind::Project,
                        revision: 0,
                        updated_at_ms: 0,
                        archived: false,
                    },
                    body: original.clone(),
                },
                0,
            )
            .await
            .unwrap();
            let Output::QueuedTurn(turn) = fixture
                .client
                .execute(fixture.client.prepare(Command::SubmitTurn {
                    session: fixture.session.id,
                    expected_revision: 1,
                    message: "Check the project constraint".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                page.runs[0].status,
                Status::Completed,
                "{:?}",
                page.runs[0].error
            );
            let results: Vec<_> = page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter_map(|part| match part {
                    Part::ToolResult { result, .. } => Some(result),
                    _ => None,
                })
                .collect();
            assert_eq!(results.len(), 3);
            assert_eq!(
                results
                    .iter()
                    .filter(|result| result["error"]["code"] == "invalid_request")
                    .count(),
                2
            );
            let search = results
                .iter()
                .find(|result| result.get("entries").is_some())
                .unwrap();
            let text = search["entries"][0]["parts"][0]["text"].as_str().unwrap();
            let retrieved: serde_json::Value = serde_json::from_str(text).unwrap();
            assert_eq!(retrieved["body_truncated"], !parallel);
            let saved = support::read(&fixture.client, id).await.unwrap();
            assert_eq!(saved.body, original);
            assert_eq!(saved.summary.revision, 1);
            let entries = support::list(&fixture.client).await.unwrap();
            assert_eq!(entries.len(), 1);
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn leaves_tool_use_optional() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let fixture = Fixture::new(remote, &server).await;
        install(&fixture).await;
        support::save_settings(
            &fixture.client,
            support::Settings {
                auto_write: true,
                ..support::settings(&fixture.client).await.unwrap()
            },
        )
        .await
        .unwrap();
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: 1,
                message: "Hello".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert!(
            !page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| matches!(part, Part::ToolResult { .. }))
        );
        assert_eq!(support::list(&fixture.client).await.unwrap(), vec![]);
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            let prompt = requests[0]["messages"].to_string();
            assert!(prompt.contains("Memory is optional"));
            assert!(!prompt.contains("Before answering"));
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

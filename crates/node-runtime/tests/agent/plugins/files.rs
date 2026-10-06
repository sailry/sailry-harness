use super::*;
use sailry_protocol::{conversation::checkpoint, plugin};
use serde_json::{Value, json};
use std::io::{Cursor, Write as _};

fn name(tool: &str) -> String {
    plugin_tool("files", tool)
}

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

async fn install(fixture: &process::Fixture) -> plugin::Info {
    let Output::Plugins(packages) = execute(&fixture.client, Command::ListPlugins).await else {
        panic!("package inventory expected")
    };
    let revision = packages
        .iter()
        .find(|package| package.name == "files")
        .map_or(0, |package| package.revision);
    let Output::Plugin(package) = execute(
        &fixture.client,
        Command::InstallBundledPlugin {
            name: "files".into(),
            expected_revision: revision,
        },
    )
    .await
    else {
        panic!("package expected")
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    let package = if package.summary.enabled {
        package
    } else {
        let Output::Plugin(package) = execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: "files".into(),
                expected_revision: package.summary.revision,
                enabled: true,
            },
        )
        .await
        else {
            panic!("enabled package expected")
        };
        package
    };
    assert_eq!(package.extension.as_ref().unwrap().tools.len(), 6);
    package
}

async fn configure(fixture: &mut process::Fixture, permission: Permission, mode: WorkMode) {
    let mut config = fixture.session.config.clone();
    config.permission = permission;
    config.mode = mode;
    let Output::Session(session) = execute(
        &fixture.client,
        Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config,
        },
    )
    .await
    else {
        panic!("session expected")
    };
    fixture.session = session;
}

async fn submit(fixture: &process::Fixture) -> QueuedTurn {
    let Output::QueuedTurn(turn) = execute(
        &fixture.client,
        Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Use the selected file tools".into(),
        },
    )
    .await
    else {
        panic!("turn expected")
    };
    turn
}

fn results(page: &Page) -> Vec<&Value> {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| match part {
            Part::ToolResult { result, .. } => Some(result),
            _ => None,
        })
        .collect()
}

async fn checkpoints(client: &Client, session: SessionId, turn: TurnId) -> Vec<checkpoint::File> {
    let Output::FileCheckpoints(page) = execute(
        client,
        Command::ListFileCheckpoints {
            session,
            turn,
            before: None,
            limit: 100,
        },
    )
    .await
    else {
        panic!("checkpoints expected")
    };
    assert!(page.next.is_none());
    page.files
}

async fn completed(client: &Client, request: &Request) -> Output {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let RequestOutcome::Completed(result) = client.outcome(request).await.unwrap() {
                break result.unwrap();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn checkpoints_project_writes_and_recovers_receipts() {
    for remote in [false, true] {
        let revision = |text: &str| blake3::hash(text.as_bytes()).to_hex().to_string();
        let server = Server::tools(vec![
            (
                name("write_file"),
                json!({"path":"note.txt","text":"First","expected_revision":revision("Original")}),
            ),
            (
                name("write_file"),
                json!({"path":"note.txt","text":"Second","expected_revision":revision("First")}),
            ),
            (
                name("write_file"),
                json!({"path":"note.txt","text":"Stale","expected_revision":revision("Original")}),
            ),
        ])
        .await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        let package = install(&fixture).await;
        std::fs::write(fixture.root.join("note.txt"), "Original").unwrap();
        configure(&mut fixture, Permission::Project, WorkMode::Code).await;
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        assert_eq!(page.approvals.len(), 3);
        assert!(
            page.approvals
                .iter()
                .all(|approval| approval.source == ApprovalSource::Project
                    && approval.state == ApprovalState::Approved)
        );
        let outputs = results(&page);
        assert_eq!(outputs.len(), 3);
        assert_eq!(outputs[0]["kind"], "file_written");
        assert_eq!(outputs[1]["kind"], "file_written");
        assert_eq!(outputs[2]["error"]["code"], "revision_conflict");
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("note.txt")).unwrap(),
            "Second"
        );
        let files = checkpoints(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(files.len(), 2);
        for (file, before, after) in [
            (&files[0], "First", "Second"),
            (&files[1], "Original", "First"),
        ] {
            let Output::FileCheckpoint(content) = execute(
                &fixture.client,
                Command::ReadFileCheckpoint {
                    session: fixture.session.id,
                    checkpoint: file.id,
                },
            )
            .await
            else {
                panic!("checkpoint content expected")
            };
            assert_eq!(content.before.as_deref(), Some(before));
            assert_eq!(content.after, after);
            assert_eq!(file.worktree, fixture.session.worktree);
            assert!(matches!(file.outcome, RequestOutcome::Completed(_)));
        }
        let database =
            rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3"))
                .unwrap();
        let (id, body): (String, Vec<u8>) = database.query_row(
            "SELECT r.id,r.body FROM requests r JOIN agent_approvals a ON r.id=a.request WHERE a.id=?1",
            [page.approvals[0].id.to_string()],|row|Ok((row.get(0)?,row.get(1)?))
        ).unwrap();
        drop(database);
        let mut original = fixture
            .client
            .prepare(Command::WriteFile {
                worktree: fixture.session.worktree,
                path: "note.txt".into(),
                text: "First".into(),
                expected_revision: Some(revision("Original")),
            })
            .with_plugin(plugin::Context {
                invocation: None,
                turn: Some(turn.id),
                surface: plugin::desktop::Surface::Workspace,
                package: package.summary.reference(),
                worktree: Some(fixture.session.worktree),
                session: Some(fixture.session.id),
            });
        original.id = id.parse().unwrap();
        assert_eq!(
            body,
            format!(
                "file-v1:{}",
                blake3::hash(&serde_json::to_vec(&original).unwrap()).to_hex()
            )
            .into_bytes()
        );
        assert_eq!(
            original.plugin.as_ref().unwrap().package,
            package.summary.reference()
        );
        assert!(
            matches!(&original.command,Command::WriteFile {worktree,path,..} if *worktree == fixture.session.worktree && path == "note.txt")
        );
        let expected: Output = serde_json::from_value(outputs[0].clone()).unwrap();
        assert_eq!(
            fixture.client.execute(original.clone()).await.unwrap(),
            expected
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("note.txt")).unwrap(),
            "Second"
        );
        let stale = fixture.client.prepare(Command::RestoreFileCheckpoint {
            session: fixture.session.id,
            checkpoint: files[1].id,
            worktree: fixture.session.worktree,
        });
        assert_eq!(
            fixture.client.execute(stale).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        let restore = fixture.client.prepare(Command::RestoreFileCheckpoint {
            session: fixture.session.id,
            checkpoint: files[0].id,
            worktree: fixture.session.worktree,
        });
        let pending = fixture.client.dispatch(restore.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        drop(pending.completion);
        let restored = completed(&fixture.client, &restore).await;
        assert!(matches!(restored, Output::FileRestored(_)));
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("note.txt")).unwrap(),
            "First"
        );
        std::fs::write(fixture.root.join("note.txt"), "External edit").unwrap();
        assert_eq!(fixture.client.execute(restore).await.unwrap(), restored);
        assert_eq!(fixture.client.execute(original).await.unwrap(), expected);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("note.txt")).unwrap(),
            "External edit"
        );
        assert_eq!(history(&fixture.client, fixture.session.id).await, page);
        assert_eq!(
            checkpoints(&fixture.client, fixture.session.id, turn.id).await,
            files
        );
        let Output::Rewound(rewind) = execute(
            &fixture.client,
            Command::RewindConversation {
                session: fixture.session.id,
                through: None,
                expected_head: turn.id,
                expected_revision: page.revision,
            },
        )
        .await
        else {
            panic!("rewind expected")
        };
        assert_eq!(
            checkpoints(&fixture.client, rewind.backup.id, turn.id).await,
            files
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("note.txt")).unwrap(),
            "External edit"
        );
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_ask_and_plan_policy() {
    for remote in [false, true] {
        let server = Server::tools(vec![(
            name("write_file"),
            json!({"path":"approved.txt","text":"Approved","expected_revision":null}),
        )])
        .await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        install(&fixture).await;
        let turn = submit(&fixture).await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert_eq!(approval.source, ApprovalSource::User);
        assert!(!fixture.root.join("approved.txt").exists());
        assert!(
            checkpoints(&fixture.client, fixture.session.id, turn.id)
                .await
                .is_empty()
        );
        process::decide(&fixture.client, &approval, Decision::Approve).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(
            checkpoints(&fixture.client, fixture.session.id, turn.id)
                .await
                .len(),
            1
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("approved.txt")).unwrap(),
            "Approved"
        );
        configure(&mut fixture, Permission::Full, WorkMode::Plan).await;
        let planned = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, planned.id).await;
        assert_eq!(page.approvals.len(), 1);
        assert!(
            checkpoints(&fixture.client, fixture.session.id, planned.id)
                .await
                .is_empty()
        );
        let request = server.requests.lock().unwrap().last().cloned().unwrap();
        let names: Vec<_> = request["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["function"]["name"].as_str().unwrap())
            .collect();
        for read in ["list_directory", "read_file", "search_files", "read_office"] {
            assert!(names.contains(&name(read).as_str()));
        }
        for write in ["write_file", "export_pdf"] {
            assert!(!names.contains(&name(write).as_str()));
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_maximum_escaped_text() {
    for remote in [false, true] {
        let text = "\u{1}".repeat(MAX_FILE_BYTES);
        let arguments = json!({"path":"escaped.txt","text":text,"expected_revision":null});
        assert!(serde_json::to_vec(&arguments).unwrap().len() > 256 * 1024);
        let server = Server::tools(vec![
            (name("write_file"), arguments),
            (name("read_file"), json!({"path":"escaped.txt"})),
        ])
        .await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        install(&fixture).await;
        configure(&mut fixture, Permission::Project, WorkMode::Code).await;
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("escaped.txt")).unwrap(),
            text
        );
        let outputs = results(&page);
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0]["kind"], "file_written");
        assert_eq!(outputs[1]["kind"], "file_content");
        assert_eq!(outputs[1]["data"]["text"], text);
        assert_eq!(outputs[1]["data"]["truncated"], false);
        assert_eq!(
            checkpoints(&fixture.client, fixture.session.id, turn.id)
                .await
                .len(),
            1
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn confines_arguments_and_preserves_large_office_results() {
    for remote in [false, true] {
        let server = Server::tools(vec![
            (name("list_directory"), json!({"path":"."})),
            (name("read_file"), json!({"path":"note.txt"})),
            (
                name("search_files"),
                json!({"query":"needle","globs":["*.txt"]}),
            ),
            (name("read_file"), json!({"path":"../outside.txt"})),
            (
                name("read_file"),
                json!({"path":"note.txt","worktree":WorktreeId::new()}),
            ),
            (name("read_office"), json!({"path":"long.docx"})),
        ])
        .await;
        let fixture = process::Fixture::new(remote, &server).await;
        install(&fixture).await;
        std::fs::write(fixture.root.join("note.txt"), "needle in captured worktree").unwrap();
        std::fs::write(
            fixture.directory.path().join("outside.txt"),
            "Outside scope",
        )
        .unwrap();
        let text = "o".repeat(MAX_FILE_BYTES * 8);
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        archive
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive
            .write_all(format!("<document><p><t>{text}</t></p></document>").as_bytes())
            .unwrap();
        std::fs::write(
            fixture.root.join("long.docx"),
            archive.finish().unwrap().into_inner(),
        )
        .unwrap();
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        assert!(page.approvals.is_empty());
        let output = results(&page);
        assert_eq!(output.len(), 6);
        assert_eq!(output[0]["kind"], "directory");
        assert_eq!(output[1]["data"]["text"], "needle in captured worktree");
        assert_eq!(output[2]["data"]["matches"][0]["path"], "note.txt");
        for denied in [&output[3], &output[4]] {
            assert_eq!(denied["isError"], true);
            assert!(denied["error"]["code"].is_string());
            assert!(!denied.to_string().contains("Outside scope"));
        }
        assert_eq!(output[4]["error"]["code"], "invalid_request");
        assert_eq!(output[5]["kind"], "office_content");
        let Output::OfficeContent(content) = serde_json::from_value(output[5].clone()).unwrap()
        else {
            panic!("Office content expected")
        };
        assert_eq!(
            content
                .sections
                .iter()
                .map(|section| section.text.as_str())
                .collect::<String>(),
            text
        );
        assert!(content.next.is_none());
        assert!(serde_json::to_vec(output[5]).unwrap().len() > MAX_FILE_BYTES);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn restores_prepared_plugin_content_after_restart() {
    for remote in [false, true] {
        let package_name = "prepared-files";
        let tool = plugin_tool(package_name, "save");
        let arguments = json!({"text":"Original model argument"});
        let server = Server::tools(vec![(tool.clone(), arguments.clone())]).await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        let source = fixture.root.join("prepared-plugin");
        std::fs::create_dir_all(source.join(plugin::NAMESPACE)).unwrap();
        let before = "Before prepared write";
        let after = "Prepared header\nOriginal model argument\nPrepared footer";
        let revision = blake3::hash(before.as_bytes()).to_hex().to_string();
        std::fs::write(fixture.root.join("prepared.txt"), before).unwrap();
        std::fs::write(source.join("dev.sailry.platform/main.js"), format!(r#"
export function save(args) {{
  return {{path:'prepared.txt',text:'Prepared header\n'+args.text+'\nPrepared footer',expected_revision:{revision:?}}};
}}
"#)).unwrap();
        std::fs::write(source.join("plugin.json"), json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name":package_name,"version":"1.0.0","extensions":{"dev.sailry.platform":{
                "api_version":"v1","actions":["files.write"],
                "host":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],"handlers":["save"]},
                "tools":[{"name":"save","description":"Save prepared content","handler":{
                    "name":"save","operation":"files.write","parameters":{
                        "type":"object","properties":{"text":{"type":"string"}},"required":["text"],"additionalProperties":false
                    }
                }}]
            }}
        }).to_string()).unwrap();
        let Output::Plugin(package) = execute(
            &fixture.client,
            Command::InstallPlugin {
                name: package_name.into(),
                path: "prepared-plugin".into(),
                worktree: fixture.session.worktree,
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("package expected")
        };
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        configure(&mut fixture, Permission::Project, WorkMode::Code).await;
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        assert_eq!(page.approvals.len(), 1);
        assert_eq!(page.approvals[0].source, ApprovalSource::Project);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("prepared.txt")).unwrap(),
            after
        );
        assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| {
            matches!(part, Part::ToolCall {name,arguments:original,..} if name == &tool && original == &arguments)
        }));
        let files = checkpoints(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "prepared.txt");
        let checkpoint = files[0].id;
        let Output::FileCheckpoint(content) = execute(
            &fixture.client,
            Command::ReadFileCheckpoint {
                session: fixture.session.id,
                checkpoint,
            },
        )
        .await
        else {
            panic!("checkpoint expected")
        };
        assert_eq!(content.before.as_deref(), Some(before));
        assert_eq!(content.after, after);
        let Output::TurnDiff(diff) = execute(
            &fixture.client,
            Command::ReadTurnDiff {
                session: fixture.session.id,
                turn: turn.id,
            },
        )
        .await
        else {
            panic!("turn diff expected")
        };
        assert!(!diff.partial);
        assert_eq!(diff.files.len(), 1);
        assert!(diff.files[0].text.contains("+Prepared header"));
        assert!(diff.files[0].text.contains("+Prepared footer"));
        execute(
            &fixture.client,
            Command::RemovePlugin {
                name: package_name.into(),
                expected_revision: package.summary.revision,
            },
        )
        .await;
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start(profile).await.unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            execute(
                &client,
                Command::ReadFileCheckpoint {
                    session: fixture.session.id,
                    checkpoint
                }
            )
            .await,
            Output::FileCheckpoint(content)
        );
        assert_eq!(history(&client, fixture.session.id).await, page);
        let restore = client.prepare(Command::RestoreFileCheckpoint {
            session: fixture.session.id,
            checkpoint,
            worktree: fixture.session.worktree,
        });
        let pending = client.dispatch(restore.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        drop(pending.completion);
        let restored = completed(&client, &restore).await;
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("prepared.txt")).unwrap(),
            before
        );
        std::fs::write(fixture.root.join("prepared.txt"), "Later external edit").unwrap();
        assert_eq!(client.execute(restore).await.unwrap(), restored);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("prepared.txt")).unwrap(),
            "Later external edit"
        );
        assert_eq!(history(&client, fixture.session.id).await, page);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

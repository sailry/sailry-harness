use super::*;
#[path = "../../support/skills_git.rs"]
mod source;
use sailry_protocol::plugin::skills::Discovery;

struct Fixture {
    _directory: tempfile::TempDir,
    node: Node,
    controller: Link,
    client: Client,
    session: Session,
    server: Server,
    original: Discovery,
    next: Discovery,
}

impl Fixture {
    async fn new(
        remote: bool,
        git: &source::Git,
        permission: Permission,
        mode: WorkMode,
        calls: impl FnOnce(&Discovery, &Discovery) -> Vec<(String, Value)>,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        fs::create_dir(&root).unwrap();
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
        let original = discovery(&client, git.source()).await;
        git.update();
        let next = discovery(&client, git.source()).await;
        let calls = calls(&original, &next)
            .into_iter()
            .map(|(action, mut arguments)| {
                if ["list", "discover", "install", "update", "uninstall"].contains(&action.as_str())
                {
                    arguments["action"] = json!(action);
                    ("manage_skills".into(), arguments)
                } else {
                    (action, arguments)
                }
            })
            .collect();
        let server = Server::tools(calls).await;
        let (session, provider) = configured_session(&client, &server.endpoint, &root).await;
        let mut session = approvals::prepare_model(&client, session, provider).await;
        let mut config = session.config.clone();
        config.permission = permission;
        config.mode = mode;
        let Output::Session(updated) = execute(
            &client,
            Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config,
            },
        )
        .await
        else {
            panic!("session expected");
        };
        session = updated;
        Self {
            _directory: directory,
            node,
            controller,
            client,
            session,
            server,
            original,
            next,
        }
    }

    async fn submit(&self) -> QueuedTurn {
        let Output::QueuedTurn(turn) = execute(
            &self.client,
            Command::SubmitTurn {
                session: self.session.id,
                expected_revision: self.session.revision,
                message: "Manage the requested skills".into(),
            },
        )
        .await
        else {
            panic!("turn expected");
        };
        turn
    }

    async fn close(self) {
        drop(self.client);
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

async fn discovery(client: &Client, source: plugin::skills::Source) -> Discovery {
    let Output::SkillDiscovery(discovery) =
        execute(client, Command::DiscoverSkills { source }).await
    else {
        panic!("skill discovery expected");
    };
    discovery
}

fn selected(discovery: &Discovery) -> &plugin::skills::Candidate {
    discovery
        .skills
        .iter()
        .find(|skill| skill.skill.name == "analysis")
        .unwrap()
}

fn install(discovery: &Discovery) -> Value {
    let skill = selected(discovery);
    let Command::InstallSkill {
        source, path, name, ..
    } = source::install(discovery, skill, 0)
    else {
        unreachable!()
    };
    json!({"source":source,"path":path,"name":name})
}

fn update(discovery: &Discovery, revision: u64) -> Value {
    let mut args = install(discovery);
    args["expected_revision"] = json!(revision);
    args
}

fn names(request: &Value) -> Vec<&str> {
    request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn permissions_and_empty_inventory() {
    for remote in [false, true] {
        for (permission, allow) in [
            (Permission::Ask, true),
            (Permission::Ask, false),
            (Permission::Project, true),
            (Permission::Full, true),
        ] {
            let git = source::Git::new();
            let fixture = Fixture::new(remote, &git, permission, WorkMode::Code, |original, _| {
                vec![
                    ("list".into(), json!({})),
                    (
                        "discover".into(),
                        json!({"repository":git.url,"ref":"release/v1"}),
                    ),
                    ("install".into(), install(original)),
                    ("list".into(), json!({})),
                ]
            })
            .await;
            enabled_only(&fixture.client, &[]).await;
            let turn = fixture.submit().await;
            for index in 0..if permission == Permission::Full { 0 } else { 4 } {
                let (pending, approval) =
                    approvals::pending(&fixture.client, fixture.session.id).await;
                assert_eq!(approval.source, ApprovalSource::User);
                assert!(pending.entries.iter().flat_map(|entry| &entry.parts).any(
                    |part| matches!(part, Part::ToolCall { name, .. } if name == "manage_skills")
                ));
                let Output::Plugins(before) = execute(&fixture.client, Command::ListPlugins).await
                else {
                    panic!("plugins expected");
                };
                assert_eq!(
                    before
                        .iter()
                        .any(|entry| entry.name == selected(&fixture.original).name),
                    index > 2 && allow
                );
                execute(
                    &fixture.client,
                    Command::ResolveApproval {
                        session: fixture.session.id,
                        approval: approval.id,
                        decision: if index != 2 || allow {
                            Decision::Approve
                        } else {
                            Decision::Deny
                        },
                    },
                )
                .await;
            }
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
            let view = super::super::progress::observe(&fixture.client, fixture.session.id).await;
            assert_eq!(view.calls.len(), 4);
            for index in [0, 1, 3] {
                assert!(!view.calls[index].details(&page));
                assert!(view.calls[index].display(&page).unwrap().valid());
            }
            if allow {
                assert!(view.calls[2].details(&page));
            }
            let output = results(&page);
            assert_eq!(output[0]["skills"], json!([]));
            assert_eq!(output[1]["source"]["commit"], fixture.next.source.commit);
            assert_eq!(
                output[3]["skills"].as_array().unwrap().len(),
                usize::from(allow)
            );
            if allow {
                assert_eq!(output[2]["name"], selected(&fixture.original).name);
                assert_eq!(output[3]["skills"][0]["source"]["commit"], git.first);
                let expected = if permission == Permission::Full {
                    ApprovalSource::Full
                } else {
                    ApprovalSource::User
                };
                assert_eq!(page.approvals[0].source, expected);
            }
            for request in fixture.server.requests.lock().unwrap().iter() {
                let names = names(request);
                assert!(names.contains(&"manage_skills"));
                for name in [
                    "list_skills",
                    "discover_skills",
                    "install_skill",
                    "update_skill",
                    "uninstall_skill",
                ] {
                    assert!(!names.contains(&name));
                }
                assert!(!names.contains(&"load_skill"));
            }
            // Management belongs to the captured execution Node, not the controller or another Node.
            let other_directory = tempfile::tempdir().unwrap();
            let other = Node::start_empty(other_directory.path().join("other"))
                .await
                .unwrap();
            let other_client = Client::new(other.local());
            let Output::Plugins(entries) = execute(&other_client, Command::ListPlugins).await
            else {
                panic!("plugins expected");
            };
            assert!(
                !entries
                    .iter()
                    .any(|entry| entry.name == selected(&fixture.original).name)
            );
            drop(other_client);
            other.shutdown().await.unwrap();
            fixture.close().await;
        }
    }
}

#[tokio::test]
async fn preserves_identity_and_rejects_feature_packages() {
    for remote in [false, true] {
        let git = source::Git::new();
        let fixture = Fixture::new(
            remote,
            &git,
            Permission::Full,
            WorkMode::Code,
            |original, next| {
                let name = &selected(original).name;
                let mut changed = update(next, 1);
                changed["path"] = json!("other");
                let mut feature_update = update(next, 1);
                feature_update["name"] = json!("files");
                vec![
                    (
                        "uninstall".into(),
                        json!({"name":"files","expected_revision":1}),
                    ),
                    ("update".into(), feature_update),
                    ("install".into(), install(original)),
                    ("update".into(), changed),
                    ("update".into(), update(next, 1)),
                    (
                        "uninstall".into(),
                        json!({"name":name,"expected_revision":1}),
                    ),
                    (
                        "uninstall".into(),
                        json!({"name":name,"expected_revision":2}),
                    ),
                    ("list".into(), json!({})),
                ]
            },
        )
        .await;
        let Output::Plugin(files) = execute(
            &fixture.client,
            Command::InstallBundledPlugin {
                name: "files".into(),
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("files package expected");
        };
        assert!(files.issues.is_empty(), "{:?}", files.issues);
        let turn = fixture.submit().await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        let output = results(&page);
        assert_eq!(
            output[0]["error"]["code"],
            json!(ErrorCode::PermissionDenied)
        );
        assert_eq!(
            output[1]["error"]["code"],
            json!(ErrorCode::PermissionDenied)
        );
        assert_eq!(output[2]["revision"], 1);
        assert_eq!(output[3]["error"]["code"], json!(ErrorCode::InvalidRequest));
        assert_eq!(output[4]["revision"], 2);
        assert_eq!(
            output[4]["source"]["source"]["commit"],
            fixture.next.source.commit
        );
        assert_eq!(
            output[5]["error"]["code"],
            json!(ErrorCode::RevisionConflict)
        );
        assert_eq!(output[6], json!({"skills":[]}));
        assert_eq!(output[7], json!({"skills":[]}));
        let Output::Plugin(files) = execute(
            &fixture.client,
            Command::ReadPlugin {
                name: "files".into(),
            },
        )
        .await
        else {
            panic!("files package expected");
        };
        assert!(files.summary.enabled);
        assert_eq!(files.summary.revision, 1);
        fixture.close().await;
    }
}

#[tokio::test]
async fn planning_exposes_only_reads() {
    for remote in [false, true] {
        let git = source::Git::new();
        let fixture = Fixture::new(remote, &git, Permission::Full, WorkMode::Plan, |_, _| {
            vec![
                ("list".into(), json!({})),
                (
                    "discover".into(),
                    json!({"repository":git.url,"ref":"release/v1","path":"skills/analysis"}),
                ),
            ]
        })
        .await;
        let turn = fixture.submit().await;
        let page = questions::finish_plan(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        assert!(page.approvals.is_empty());
        assert_eq!(results(&page)[1]["skills"].as_array().unwrap().len(), 1);
        for request in fixture.server.requests.lock().unwrap().iter() {
            let names = names(request);
            assert!(names.contains(&"manage_skills"));
            let tool = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["function"]["name"] == "manage_skills")
                .unwrap();
            let parameters = &tool["function"]["parameters"];
            assert_eq!(
                parameters["properties"]["action"]["enum"],
                json!(["list", "discover"]),
                "{parameters}"
            );
            let schema = jsonschema::validator_for(parameters).unwrap();
            for arguments in [
                json!({"action":"list"}),
                json!({"action":"discover","repository":git.url,"ref":"release/v1","path":"skills/analysis"}),
            ] {
                assert!(schema.is_valid(&arguments), "{arguments}: {parameters}");
            }
            for (action, mut arguments) in [
                ("install", install(&fixture.original)),
                ("update", update(&fixture.next, 1)),
                (
                    "uninstall",
                    json!({"name":selected(&fixture.original).name,"expected_revision":1}),
                ),
            ] {
                arguments["action"] = json!(action);
                assert!(!schema.is_valid(&arguments), "{arguments}: {parameters}");
            }
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn keeps_admitted_resources() {
    for remote in [false, true] {
        let git = source::Git::new();
        let fixture = Fixture::new(
            remote,
            &git,
            Permission::Full,
            WorkMode::Code,
            |original, next| {
                let name = &selected(original).name;
                let skill = format!("{name}:analysis");
                vec![
                    ("update".into(), update(next, 1)),
                    (
                        "read_skill_resource".into(),
                        json!({"skill":skill,"path":"references/guide.md"}),
                    ),
                    (
                        "uninstall".into(),
                        json!({"name":name,"expected_revision":2}),
                    ),
                    ("load_skill".into(), json!({"skill":skill})),
                ]
            },
        )
        .await;
        let Output::Plugin(installed) = execute(
            &fixture.client,
            source::install(&fixture.original, selected(&fixture.original), 0),
        )
        .await
        else {
            panic!("skill package expected");
        };
        let turn = fixture.submit().await;
        assert!(turn.plugins.contains(&installed.summary.reference()));
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        let output = results(&page);
        assert_eq!(output[0]["revision"], 2);
        assert_eq!(output[1]["content"], "Guide first 中文 🙂");
        assert_eq!(output[2], json!({"skills":[]}));
        assert_eq!(output[3]["content"], source::body("first"));
        let Output::Plugins(entries) = execute(&fixture.client, Command::ListPlugins).await else {
            panic!("plugins expected");
        };
        assert!(
            !entries
                .iter()
                .any(|entry| entry.name == installed.summary.name)
        );
        let Output::Plugin(retained) = execute(
            &fixture.client,
            Command::ReadPluginVersion {
                package: installed.summary.reference(),
            },
        )
        .await
        else {
            panic!("retained skill package expected");
        };
        assert_eq!(retained.skill, installed.skill);
        fixture.close().await;
    }
}

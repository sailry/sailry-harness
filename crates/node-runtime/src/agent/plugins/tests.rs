use super::*;
use sailry_protocol::{
    Permission, ProviderId, QueuedTurn, RequestId, SessionConfig, WorkMode, WorktreeId,
    conversation::RunKind,
};
use serde_json::json;

#[tokio::test]
async fn registers_independent_reads_as_concurrent() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("package");
    let profile = directory.path().join("plugins");
    std::fs::create_dir_all(source.join("dev.sailry.platform/host")).unwrap();
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(
        source.join("dev.sailry.platform/host/main.js"),
        "export function prepare(input) { return input; }",
    )
    .unwrap();
    let tools = [
        ("read_file", json!({"operation":"files.read"})),
        ("edit_file", json!({"operation":"files.edit"})),
        (
            "read_pair",
            json!({"flow":{"operations":["files.read","files.search"]}}),
        ),
        (
            "write_pair",
            json!({"flow":{"operations":["files.read","files.mkdir"]}}),
        ),
        ("script", json!({"read_only":true})),
    ]
    .map(|(name, binding)| {
        let mut handler = json!({"name":"prepare","parameters":{"type":"object"}});
        handler
            .as_object_mut()
            .unwrap()
            .extend(binding.as_object().unwrap().clone());
        json!({"name":name,"description":"Exercise file operations","handler":handler})
    });
    std::fs::write(
        source.join("plugin.json"),
        json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name":"files","version":"0.1.0","extensions":{"dev.sailry.platform":{
                "api_version":"v1","actions":["files.read","files.write"],
                "host":{"entry":"dev.sailry.platform/host/main.js",
                    "resources":["dev.sailry.platform/host/main.js"],"handlers":["prepare"]},
                "tools":tools
            }}
        })
        .to_string(),
    )
    .unwrap();
    let host = crate::plugins::Host::new(Some(profile.canonicalize().unwrap()));
    let package = host
        .install(&source.canonicalize().unwrap(), "", "files")
        .unwrap();
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    let node = crate::Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let invocation = Invocation {
        compaction_percent: 80,
        child_concurrency: sailry_protocol::team::CHILD_CONCURRENCY,
        instructions: vec![],
        hooks: Default::default(),
        automatic: false,
        notice: false,
        notice_input: None,
        notices: vec![],
        title: None,
        connections: crate::store::agent::connections::Catalog {
            bound: None,
            databases: vec![],
            ssh: vec![],
        },
        turn: QueuedTurn {
            id: TurnId::new(),
            kind: RunKind::Task,
            session: SessionId::new(),
            request: RequestId::new(),
            revision: 1,
            config: SessionConfig {
                assistant: None,
                resource: None,
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: Default::default(),
                features: Default::default(),
                mode: WorkMode::Code,
                options: Default::default(),
                permission: Permission::Ask,
                credential: None,
            },
            roles: Default::default(),
            plugins: vec![package.summary.reference()],
        },
        caller: node.id(),
        worktree: WorktreeId::new(),
        project: None,
        provider: None,
        media: Default::default(),
        message: Default::default(),
        input_turn: None,
        followups: vec![],
        child: None,
        plugins: vec![package],
        plugin_settings: Default::default(),
    };
    let stop = CancellationToken::new();
    let supervisor = crate::tasks::Supervisor::start(4);
    let mut bound = scripts::bind(&node.ingress, &supervisor.tasks, &invocation, &stop)
        .await
        .unwrap();
    assert_eq!(bound.tools.len(), 5);
    bound
        .tools
        .extend(references::bind(&node.ingress, &invocation, &stop));
    selection::defer(&invocation, &mut bound.tools).unwrap();
    for name in references::NAMES {
        let registration = bound
            .tools
            .iter()
            .find(|entry| entry.tool.name.as_ref() == name)
            .unwrap();
        assert!(registration.concurrent, "{name}");
        assert_eq!(registration.deferred, name != references::OUTPUT, "{name}");
    }
    let search = search::bind(&invocation, &stop, &bound.tools, &BTreeSet::new()).unwrap();
    bound.tools.push(search);
    let mut catalog = catalog::Catalog::new(invocation.turn.session.to_string(), stop, None);
    for registration in bound.tools {
        catalog.register(registration).unwrap();
    }
    for (name, expected) in [
        ("read_file", true),
        ("edit_file", false),
        ("read_pair", true),
        ("write_pair", false),
        ("script", false),
    ] {
        let call = rmcp::model::CallToolRequestParams::new(operations::alias("files", name));
        assert_eq!(catalog.concurrent(&call), expected, "{name}");
    }
    assert!(catalog.concurrent(&rmcp::model::CallToolRequestParams::new(search::NAME)));
    drop(catalog);
    supervisor.shutdown(Duration::from_secs(1)).await.unwrap();
    node.shutdown().await.unwrap();
}

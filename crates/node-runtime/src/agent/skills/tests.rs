use super::*;

#[tokio::test]
async fn binds_captured_resources_without_plugin_identity() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("profile");
    let source = directory.path().join("source");
    std::fs::create_dir(&profile).unwrap();
    std::fs::create_dir_all(source.join("skills/analysis")).unwrap();
    std::fs::write(
        source.join("plugin.json"),
        json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"example","version":"1.0.0"}).to_string(),
    ).unwrap();
    std::fs::write(
        source.join("skills/analysis/SKILL.md"),
        "---\nname: analysis\ndescription: Analyze project data\n---\nLoad this content on demand\n",
    ).unwrap();
    let host = crate::plugins::Host::new(Some(profile.canonicalize().unwrap()));
    let package = host
        .install(&source.canonicalize().unwrap(), "", "example")
        .unwrap();
    assert_eq!(package.skills.len(), 1);
    let stop = CancellationToken::new();
    let node = crate::Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let turn = sailry_protocol::TurnId::new();
    let empty = Arc::new(Resources::new(
        host.clone(),
        vec![],
        Default::default(),
        stop.clone(),
    ));
    let (tools, instruction) = bind(
        empty,
        sailry_protocol::SessionId::new(),
        stop.clone(),
        node.ingress.clone(),
        turn,
    )
    .unwrap();
    assert!(tools.is_empty());
    assert!(instruction.is_empty());
    let resources = Arc::new(Resources::new(
        host,
        vec![package],
        Default::default(),
        stop.clone(),
    ));
    let (tools, instruction) = bind(
        resources,
        sailry_protocol::SessionId::new(),
        stop,
        node.ingress.clone(),
        turn,
    )
    .unwrap();
    assert_eq!(
        tools
            .iter()
            .map(|entry| entry.tool.name.as_ref())
            .collect::<Vec<_>>(),
        NAMES
    );
    assert!(
        tools
            .iter()
            .all(|entry| entry.managed && entry.plugin.is_none())
    );
    assert!(tools.iter().all(|entry| entry.presentation
        == sailry_protocol::tool::Presentation::Summary
        && entry.grouping == sailry_protocol::tool::Grouping::Sequence));
    for entry in &tools {
        let schema = serde_json::Value::Object((*entry.tool.input_schema).clone());
        assert_eq!(
            schema["properties"]["skill"]["enum"],
            if entry.tool.name.as_ref() == NAMES[1] {
                serde_json::Value::Null
            } else {
                json!(["example:analysis"])
            }
        );
        let (name, display) = entry.display.as_ref().unwrap();
        assert_eq!(name, entry.tool.name.as_ref());
        assert!(display.valid());
        assert_eq!(display, Skills::display(name == NAMES[1]));
        let input = display.input.as_ref().unwrap();
        assert_eq!(
            input.summary.as_ref().unwrap().read(&json!({
                "skill":"example:analysis", "path":"references/guide.md"
            })),
            Some("example:analysis")
        );
        assert!(input.content.is_none());
        assert_eq!(display.locales.len(), 9);
        assert_ne!(display.label("zh-CN"), display.label("en"));
    }
    assert!(tools.iter().all(|entry| entry.read_only()));
    assert!(instruction.contains("example:analysis"));
    assert!(!instruction.contains("Load this content on demand"));
    node.shutdown().await.unwrap();
}

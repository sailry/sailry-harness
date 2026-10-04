use super::*;

#[test]
fn binds_captured_resources_without_plugin_identity() {
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
    let empty = Arc::new(Resources::new(
        host.clone(),
        vec![],
        Default::default(),
        stop.clone(),
    ));
    let (tools, instruction) =
        bind(empty, sailry_protocol::SessionId::new(), stop.clone()).unwrap();
    assert!(tools.is_empty());
    assert!(instruction.is_empty());
    let resources = Arc::new(Resources::new(
        host,
        vec![package],
        Default::default(),
        stop.clone(),
    ));
    let (tools, instruction) = bind(resources, sailry_protocol::SessionId::new(), stop).unwrap();
    assert_eq!(
        tools
            .iter()
            .map(|entry| entry.tool.name())
            .collect::<Vec<_>>(),
        NAMES
    );
    assert!(
        tools
            .iter()
            .all(|entry| entry.managed && entry.plugin.is_none())
    );
    assert!(tools.iter().all(|entry| entry.presentation
        == sailry_protocol::tool::Presentation::Details
        && entry.grouping == sailry_protocol::tool::Grouping::Sequence));
    assert!(
        tools
            .iter()
            .all(|entry| entry.tool.is_read_only() && entry.tool.is_concurrency_safe())
    );
    assert!(instruction.contains("example:analysis"));
    assert!(!instruction.contains("Load this content on demand"));
}

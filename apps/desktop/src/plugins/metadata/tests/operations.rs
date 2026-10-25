use super::*;
use sailry_protocol::{
    DatabaseId, ProjectId, connection::Resource, plugin::conversation::Binding, tool::Operation,
};
use serde_json::{Value, json};

fn package(name: &str, tools: Value) -> Info {
    let mut package = info(name);
    package.extension = Some(
        serde_json::from_value(json!({
            "api_version":"v1", "actions":["files.read", "code.index.read"],
            "tools":tools
        }))
        .unwrap(),
    );
    package
}

fn loaded(cache: &mut Metadata, package: Info) {
    cache.reconcile(std::slice::from_ref(&package.summary));
    assert!(cache.apply(
        Batch {
            results: vec![(package.summary.name.clone(), Ok(package))],
            diagnostics: Default::default(),
            retrying: false,
        },
        &cache.stop.clone(),
    ));
}

#[test]
fn preserves_order_and_constraints() {
    let mut cache = cache();
    let mut disabled = package(
        "a-disabled",
        json!([
            {"name":"read_file", "operation":"files.read"},
            {"name":"outline", "operation":"code.outline"}
        ]),
    );
    disabled.summary.enabled = false;
    let enabled = package(
        "b-enabled",
        json!([
            {"name":"invalid-name", "operation":"files.read_lines"},
            {"name":"search", "operation":"code.search"},
            {"name":"read_file", "operation":"files.read"},
            {"name":"outline", "operation":"code.outline"},
            {"name":"platform_only", "operation":"files.read_lines", "platforms":["linux"]},
            {"name":"write_file", "operation":"files.write"}
        ]),
    );
    cache.entries = [disabled, enabled]
        .into_iter()
        .map(|package| (package.summary.name.clone(), package))
        .collect();
    assert_eq!(
        cache.operations(None, None, None),
        [
            Operation::ReadFile,
            Operation::OutlineCode,
            Operation::SearchCode
        ]
    );
    let cached = cache.operations.borrow();
    assert!(cached.is_some());
    // A normal view notification must not rebuild or mutate the projection.
    assert_eq!(
        cache.operations(None, None, None),
        [
            Operation::ReadFile,
            Operation::OutlineCode,
            Operation::SearchCode
        ]
    );
}

#[test]
fn follows_session_context_and_assistant() {
    let mut cache = cache();
    let mut package = package(
        "files",
        json!([
            {"name":"read_file", "operation":"files.read", "contexts":["plugin"]},
            {"name":"outline", "operation":"code.outline", "contexts":["workspace"]},
            {"name":"search", "operation":"code.search", "contexts":["database"]},
            {"name":"read_lines", "operation":"files.read_lines"}
        ]),
    );
    package.extension.as_mut().unwrap().desktop = Some(
        serde_json::from_value(json!({"conversations":[
            {"id":"lines", "resource":"database", "context":"", "tools":[
                {"kind":"plugin", "name":"read_lines", "server":null}
            ]},
            {"id":"search", "resource":"database", "context":"", "tools":[
                {"kind":"package", "package":"files", "name":"search"}
            ]}
        ]}))
        .unwrap(),
    );
    let mut assistant = Binding {
        package: package.summary.reference(),
        id: "lines".into(),
    };
    loaded(&mut cache, package);
    assert_eq!(
        cache.operations(None, None, None),
        [Operation::ReadFile, Operation::ReadFileLines]
    );
    assert_eq!(
        cache.operations(Some(ProjectId::new()), None, None),
        [Operation::OutlineCode, Operation::ReadFileLines]
    );
    let resource = Some(Resource::Database(DatabaseId::new()));
    assert_eq!(
        cache.operations(None, resource, None),
        [Operation::SearchCode]
    );
    assert_eq!(
        cache.operations(None, resource, Some(&assistant)),
        [Operation::ReadFileLines]
    );
    assistant.id = "search".into();
    assert_eq!(
        cache.operations(None, resource, Some(&assistant)),
        [Operation::SearchCode]
    );
    assistant.package.digest = "outdated-digest".into();
    assert!(
        cache
            .operations(None, resource, Some(&assistant))
            .is_empty()
    );
}

#[test]
fn refreshes_inventory_and_loaded_metadata() {
    let mut cache = cache();
    let mut package = package(
        "files",
        json!([{ "name":"read_file", "operation":"files.read" }]),
    );
    loaded(&mut cache, package.clone());
    assert_eq!(cache.operations(None, None, None), [Operation::ReadFile]);
    assert!(!cache.reconcile(std::slice::from_ref(&package.summary)));
    assert!(cache.operations.borrow().is_some());

    package.summary.settings_revision += 1;
    package.summary.revision += 1;
    assert!(cache.reconcile(std::slice::from_ref(&package.summary)));
    assert!(cache.operations.borrow().is_none());
    assert_eq!(cache.operations(None, None, None), [Operation::ReadFile]);
    loaded(&mut cache, package.clone());
    assert!(cache.operations.borrow().is_none());
    assert_eq!(cache.operations(None, None, None), [Operation::ReadFile]);

    package.summary.enabled = false;
    assert!(cache.reconcile(std::slice::from_ref(&package.summary)));
    assert!(cache.operations(None, None, None).is_empty());
    loaded(&mut cache, package.clone());
    assert!(cache.operations(None, None, None).is_empty());

    package.summary.enabled = true;
    package.summary.digest = "updated-digest".into();
    package.summary.revision += 1;
    package.extension.as_mut().unwrap().tools =
        serde_json::from_value(json!([{ "name":"read_lines", "operation":"files.read_lines" }]))
            .unwrap();
    loaded(&mut cache, package);
    assert_eq!(
        cache.operations(None, None, None),
        [Operation::ReadFileLines]
    );
}

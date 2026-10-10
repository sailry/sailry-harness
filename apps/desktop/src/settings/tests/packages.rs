//! Test probes wrap real Kit hit targets without replacing package behavior.
use sailry_client::Client;
use sailry_protocol::{Command, Output};
use std::path::Path;

pub(super) async fn install(client: &Client, directory: &Path, name: &str) {
    let root = directory.join(format!("{name}-fixture"));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    client
        .execute(client.prepare(Command::RegisterProject {
            name: "Settings fixture".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    let worktree = snapshot
        .worktrees
        .iter()
        .find(|tree| tree.path == root.to_str().unwrap())
        .unwrap()
        .id;
    let package = root.join("package");
    crate::plugins::fixture::copy_package(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins")
            .join(name),
        &package,
    );
    let path = package.join("dev.sailry.platform/desktop/view.js");
    let source = std::fs::read_to_string(&path)
        .unwrap()
        .replace(
            "div().id(id).child(new Button",
            "Anchor.new(id,{disabled}).child(new Button",
        )
        .replace(
            "div().id(id).child(new Switch",
            "Anchor.new(id,{disabled:locked}).child(new Switch",
        )
        .replace(
            "div().id(`code-enabled-${name}`).child",
            "Anchor.new(`code-enabled-${name}`,{disabled:locked}).child",
        )
        .replace(
            "div().id(`code-command-${index}`)",
            "Anchor.new(`code-command-${index}`)",
        )
        .replace(
            "div().id('search-editor-endpoint').child",
            "Anchor.new('search-editor-endpoint').child",
        )
        .replace(
            "div().id('search-editor-key').child",
            "Anchor.new('search-editor-key').child",
        )
        .replace(
            "div().id(`search-editor-${key}`).child",
            "Anchor.new(`search-editor-${key}`).child",
        );
    std::fs::write(
        path,
        format!("import {{Anchor}} from 'sailry/test';\n{source}"),
    )
    .unwrap();
    let Output::Plugin(previous) = client
        .execute(client.prepare(Command::ReadPlugin { name: name.into() }))
        .await
        .unwrap()
    else {
        panic!("package expected")
    };
    let Output::Plugin(installed) = client
        .execute(client.prepare(Command::InstallPlugin {
            worktree,
            path: "package".into(),
            name: name.into(),
            expected_revision: previous.summary.revision,
        }))
        .await
        .unwrap()
    else {
        panic!("package expected")
    };
    assert!(installed.issues.is_empty(), "{:?}", installed.issues);
}

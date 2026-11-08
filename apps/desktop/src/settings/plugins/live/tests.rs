use super::*;
use core::prelude::v1::test;
use sailry_protocol::NodeId;

fn command(name: &str) -> Command {
    Command::SetPluginEnabled {
        name: name.into(),
        expected_revision: 1,
        enabled: true,
    }
}

#[test]
fn refreshes_unchanged_packages_on_cursor_changes() {
    use std::collections::{BTreeMap, BTreeSet};
    let package = Summary {
        compatibility: Default::default(),
        name: "fixture".into(),
        revision: 1,
        digest: "digest".into(),
        settings_revision: 1,
        enabled: true,
        version: None,
        description: None,
    };
    let mut catalog = Catalog::default();
    let mut cursors = BTreeMap::from([("fixture".into(), 1)]);
    assert!(
        catalog
            .changed(std::slice::from_ref(&package), &cursors)
            .is_empty()
    );
    catalog.packages = vec![package.clone()];
    assert!(
        catalog
            .changed(std::slice::from_ref(&package), &cursors)
            .is_empty()
    );
    cursors.insert("fixture".into(), 2);
    assert_eq!(
        catalog.changed(std::slice::from_ref(&package), &cursors),
        BTreeSet::from(["fixture".into()]),
    );
    let changed = Summary {
        revision: 2,
        ..package
    };
    cursors.insert("fixture".into(), 3);
    assert!(catalog.changed(&[changed], &cursors).is_empty());
}

#[test]
fn admits_independent_packages_without_replaying_uncertain_requests() {
    let binding = crate::settings::test_support::binding(NodeId([1; 32]));
    let mut catalog = Catalog::default();
    assert!(catalog.admit("first".into(), binding.clone(), command("first")));
    let first = catalog.start("first").unwrap();
    let id = first.1.id;
    catalog.actions.get_mut("first").unwrap().pending = false;
    assert!(catalog.admit("second".into(), binding.clone(), command("second")));
    let second = catalog.start("second").unwrap();
    assert_ne!(second.1.id, id);
    assert!(!catalog.actions["first"].pending);
    assert!(!catalog.admit("first".into(), binding, command("first")));
    let retry = catalog.start("first").unwrap();
    assert_eq!(retry.1.id, id);
    assert!(catalog.start("second").is_none());
}

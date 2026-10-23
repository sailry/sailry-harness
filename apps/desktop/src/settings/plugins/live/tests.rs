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

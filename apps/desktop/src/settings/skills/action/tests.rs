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
fn keeps_package_and_node_requests_independent() {
    let first = crate::settings::test_support::binding(NodeId([1; 32]));
    let second = crate::settings::test_support::binding(NodeId([2; 32]));
    let mut actions = BTreeMap::new();
    let first_key = admit(&mut actions, first.clone(), command("first")).unwrap();
    let original = start(&mut actions, &first_key).unwrap().1;
    let second_key = admit(&mut actions, first.clone(), command("second")).unwrap();
    assert!(start(&mut actions, &second_key).is_some());
    let other_key = admit(&mut actions, second, command("first")).unwrap();
    assert!(start(&mut actions, &other_key).is_some());
    assert_eq!(actions.len(), 3);
    actions.get_mut(&first_key).unwrap().pending = false;
    assert!(admit(&mut actions, first, command("first")).is_none());
    let retry = start(&mut actions, &first_key).unwrap().1;
    assert_eq!(retry.id, original.id);
    assert!(start(&mut actions, &first_key).is_none());
}

use super::*;
use sailry_protocol::{RoleId, role};

#[test]
fn updates_identity_and_order() {
    let mut projection = Projection::new(NodeId([1; 32]), 1);
    projection.apply(1, Update::Snapshot(snapshot(0))).unwrap();
    let mut role = role::Profile {
        appearance: None,
        id: RoleId::new(),
        revision: 1,
        key: "review".into(),
        name: "Review".into(),
        description: "Review changes".into(),
        model: None,
        max_turns: None,
        skills: vec![],
        instructions: String::new(),
    };
    let mut other = role.clone();
    other.id = RoleId::new();
    other.key = "analysis".into();
    let update = |cursor, event| {
        Update::Event(EventEnvelope {
            node: NodeId([1; 32]),
            cursor,
            event,
        })
    };
    projection
        .apply(1, update(1, Event::RoleChanged(role.clone())))
        .unwrap();
    projection
        .apply(1, update(2, Event::RoleChanged(other.clone())))
        .unwrap();
    assert_eq!(
        projection.snapshot().unwrap().roles,
        [other.clone(), role.clone()]
    );
    role.key = "a".into();
    role.revision = 2;
    let renamed = update(3, Event::RoleChanged(role.clone()));
    projection.apply(1, renamed.clone()).unwrap();
    assert_eq!(projection.apply(1, renamed).unwrap(), Apply::Ignored);
    assert_eq!(
        projection.snapshot().unwrap().roles,
        [role.clone(), other.clone()]
    );
    projection
        .apply(1, update(4, Event::RoleRemoved { id: role.id }))
        .unwrap();
    assert_eq!(projection.snapshot().unwrap().roles, [other]);
}

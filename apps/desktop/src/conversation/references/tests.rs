use super::*;
use crate::{settings::Role, workspace};
use core::prelude::v1::test;

#[test]
fn token_boundaries() {
    for text in ["@", "Please @files", "Unicode \u{4e2d}\u{6587} @roles"] {
        let trigger = Trigger::parse(text.into(), text.len()..text.len()).unwrap();
        assert_eq!(&text[trigger.range], format!("@{}", trigger.query));
    }
    for text in ["email@example.com", "@@roles", "text @two words", "@files "] {
        assert!(
            Trigger::parse(text.into(), text.len()..text.len()).is_none(),
            "{text}"
        );
    }
    assert!(Trigger::parse("@files".into(), 1..4).is_none());
    assert!(Trigger::parse("@files".into(), 2..2).is_none());
    assert!(Trigger::parse("@\u{4e2d}".into(), 2..2).is_none());
    assert_eq!(
        Trigger::parse("text @fi rest".into(), 8..8).unwrap().range,
        5..8
    );
}

#[test]
fn scoped_catalog_and_directory() {
    let mut workspace = workspace::State::default();
    let owner = workspace.owner(0);
    let key = workspace.create_session(owner);
    let roles = vec![Role::example()];
    let sessions = catalog::items(&Page::Sessions, owner, &workspace, &roles, "");
    assert!(sessions.iter().any(|item| matches!(item, Item::Reference(Reference { kind: Kind::Session(target), .. }, _) if *target == key)));
    assert!(sessions.iter().all(|item| matches!(item, Item::Reference(reference, _) if reference.owner == owner && reference.valid(owner, &workspace, &roles))));
    let file = catalog::items(&Page::Files("docs".into()), owner, &workspace, &roles, "");
    assert!(
        matches!(&file[0], Item::Reference(Reference {kind: Kind::Directory(path), ..}, _) if path == "docs")
    );
    assert!(matches!(
        &file[1],
        Item::Reference(
            Reference {
                kind: Kind::File(path),
                ..
            },
            _
        ) if path == "docs/example.md"
    ));
    assert_eq!(
        catalog::items(&Page::Root, owner, &workspace, &roles, "AGENT").len(),
        1
    );
    assert!(catalog::items(&Page::Agents, owner, &workspace, &roles, "not-found").is_empty());
    workspace.sessions.get_mut(&key).unwrap().archived = true;
    assert!(catalog::items(&Page::Sessions, owner, &workspace, &roles, "").len() < sessions.len());
}

#[test]
fn immutable_role_and_owner() {
    let workspace = workspace::State::default();
    let owner = workspace.owner(0);
    let mut roles = vec![Role::example()];
    let reference = Reference {
        owner,
        label: "@preview-reviewer".into(),
        kind: Kind::Agent(Box::new(roles[0].clone())),
    };
    assert!(reference.valid(owner, &workspace, &roles));
    assert!(!reference.valid(workspace.owner(1), &workspace, &roles));
    roles[0].instructions = "Changed instructions".into();
    assert!(!reference.valid(owner, &workspace, &roles));
    let Kind::Agent(snapshot) = &reference.kind else {
        panic!("missing role snapshot")
    };
    assert!(snapshot.instructions.is_empty());
}

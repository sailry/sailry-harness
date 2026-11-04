use super::*;
use crate::{settings::Role, workspace};
use core::prelude::v1::test;

#[test]
fn boundaries_and_ownership() {
    for (scenario, check) in [
        (
            "token_boundaries",
            (|_scenario: &str| {
                for text in ["@", "Please @files", "Unicode \u{4e2d}\u{6587} @roles"] {
                    let trigger = Trigger::parse(text.into(), text.len()..text.len()).unwrap();
                    assert_eq!(
                        &text[trigger.range],
                        format!("@{}", trigger.query),
                        "token boundaries"
                    );
                }
                for text in ["email@example.com", "@@roles", "text @two words", "@files "] {
                    assert!(
                        Trigger::parse(text.into(), text.len()..text.len()).is_none(),
                        "token boundaries: {text}"
                    );
                }
                assert!(
                    Trigger::parse("@files".into(), 1..4).is_none(),
                    "token boundaries"
                );
                assert!(
                    Trigger::parse("@files".into(), 2..2).is_none(),
                    "token boundaries"
                );
                assert!(
                    Trigger::parse("@\u{4e2d}".into(), 2..2).is_none(),
                    "token boundaries"
                );
                assert_eq!(
                    Trigger::parse("text @fi rest".into(), 8..8).unwrap().range,
                    5..8,
                    "token boundaries"
                );
            }) as fn(&str),
        ),
        (
            "scoped_catalog_and_directory",
            (|_scenario: &str| {
                let mut workspace = workspace::State::default();
                let owner = workspace.owner(0);
                let key = workspace.create_session(owner);
                let roles = vec![Role::example()];
                let sessions = catalog::items(&Page::Sessions, owner, &workspace, &roles, "");
                assert!(sessions.iter().any(|item| matches!(item, Item::Reference(Reference { kind: Kind::Session(target), .. }, _) if *target == key)), "scoped catalog and directory");
                assert!(sessions.iter().all(|item| matches!(item, Item::Reference(reference, _) if reference.owner == owner && reference.valid(owner, &workspace, &roles))), "scoped catalog and directory");
                let file =
                    catalog::items(&Page::Files("docs".into()), owner, &workspace, &roles, "");
                assert!(
                    matches!(&file[0], Item::Reference(Reference {kind: Kind::Directory(path), ..}, _) if path == "docs"),
                    "scoped catalog and directory"
                );
                assert!(
                    matches!(
                        &file[1],
                        Item::Reference(
                            Reference {
                                kind: Kind::File(path),
                                ..
                            },
                            _
                        ) if path == "docs/example.md"
                    ),
                    "scoped catalog and directory"
                );
                assert_eq!(
                    catalog::items(&Page::Root, owner, &workspace, &roles, "AGENT").len(),
                    1,
                    "scoped catalog and directory"
                );
                assert!(
                    catalog::items(&Page::Agents, owner, &workspace, &roles, "not-found")
                        .is_empty(),
                    "scoped catalog and directory"
                );
                workspace.sessions.get_mut(&key).unwrap().archived = true;
                assert!(
                    catalog::items(&Page::Sessions, owner, &workspace, &roles, "").len()
                        < sessions.len(),
                    "scoped catalog and directory"
                );
            }),
        ),
        (
            "immutable_role_and_owner",
            (|_scenario: &str| {
                let workspace = workspace::State::default();
                let owner = workspace.owner(0);
                let mut roles = vec![Role::example()];
                let reference = Reference {
                    owner,
                    label: "@preview-reviewer".into(),
                    kind: Kind::Agent(Box::new(roles[0].clone())),
                };
                assert!(
                    reference.valid(owner, &workspace, &roles),
                    "immutable role and owner"
                );
                assert!(
                    !reference.valid(workspace.owner(1), &workspace, &roles),
                    "immutable role and owner"
                );
                roles[0].instructions = "Changed instructions".into();
                assert!(
                    !reference.valid(owner, &workspace, &roles),
                    "immutable role and owner"
                );
                let Kind::Agent(snapshot) = &reference.kind else {
                    panic!("missing role snapshot")
                };
                assert!(snapshot.instructions.is_empty(), "immutable role and owner");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

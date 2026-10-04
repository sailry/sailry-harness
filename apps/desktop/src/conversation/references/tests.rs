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

#[gpui::test]
fn ignores_stale_callbacks(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
    let mut shell = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Shell::new(window, cx));
        shell = Some(view.clone());
        gpui_kit::component::Root::new(view, window, cx)
    });
    let shell = shell.unwrap();
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let key = (0, 2);
            shell.conversations[&key]
                .input
                .clone()
                .update(cx, |input, cx| input.set_value("", window, cx));
            shell.refresh_references(key, window, cx);
            shell.conversations[&key]
                .input
                .clone()
                .update(cx, |input, cx| {
                    input.set_value("@", window, cx);
                    input.set_selected_range(1..1, cx);
                });
            shell.refresh_references(key, window, cx);
            let state = &shell.conversations[&key].references;
            let generation = state.generation;
            let row = state.list.read(cx).delegate().rows[0].clone();
            shell
                .conversations
                .get_mut(&key)
                .unwrap()
                .references
                .dismiss();
            shell.conversations[&key]
                .input
                .clone()
                .update(cx, |input, cx| input.set_value("", window, cx));
            shell.refresh_references(key, window, cx);
            shell.conversations[&key]
                .input
                .clone()
                .update(cx, |input, cx| {
                    input.set_value("@", window, cx);
                    input.set_selected_range(1..1, cx);
                });
            shell.refresh_references(key, window, cx);
            shell.choose_reference(key, generation, row, window, cx);
            assert!(shell.conversations[&key].references.path.is_empty());
            let state = &shell.conversations[&key].references;
            let generation = state.generation;
            let row = state.list.read(cx).delegate().rows[0].clone();
            shell.select_composer_host(1, window, cx);
            shell.choose_reference(key, generation, row, window, cx);
            assert!(shell.conversations[&key].options.references.is_empty());
            assert_eq!(shell.conversations[&key].input.read(cx).value(), "@");
        });
    });
}

#[gpui::test]
fn preview_selection_uses_native_tokens_and_undo(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
    let mut shell = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Shell::new(window, cx));
        shell = Some(view.clone());
        gpui_kit::component::Root::new(view, window, cx)
    });
    let shell = shell.unwrap();
    let key = (0, 2);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let input = shell.conversations[&key].input.clone();
            input.update(cx, |input, cx| {
                input.set_value("@", window, cx);
                input.set_selected_range(1..1, cx);
            });
            shell.refresh_references(key, window, cx);
            let generation = shell.conversations[&key].references.generation;
            let reference = Reference {
                owner: shell.workspace.sessions[&key].owner,
                label: "README.md".into(),
                kind: Kind::File("README.md".into()),
            };
            shell.choose_reference(
                key,
                generation,
                Item::Reference(reference.clone(), "".into()),
                window,
                cx,
            );
            assert_eq!(input.read(cx).value(), "@README.md ");
            assert_eq!(input.read(cx).tokens().len(), 1);
            assert_eq!(
                active(
                    &input.read(cx).content(),
                    &shell.conversations[&key].options.references
                ),
                [reference]
            );
        });
    });
    visual.simulate_keystrokes("cmd-a backspace");
    assert!(shell.read_with(visual, |shell, cx| {
        let thread = &shell.conversations[&key];
        active(&thread.input.read(cx).content(), &thread.options.references).is_empty()
    }));
    visual.simulate_keystrokes("cmd-z");
    assert_eq!(
        shell.read_with(visual, |shell, cx| shell.conversations[&key]
            .input
            .read(cx)
            .tokens()
            .len()),
        1
    );
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.conversations[&key]
            .options
            .references
            .len()),
        1
    );
    visual.update(|window, _| window.remove_window());
}

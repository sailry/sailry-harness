use super::*;
use core::prelude::v1::test;
use gpui_kit::component::WindowExt;
mod fixture;
mod lifecycle;
mod location;
#[cfg(target_os = "macos")]
mod native;
use fixture::{Fixture, after, before, copied, init, open_file, restore, start, tap, wait};

#[gpui::test]
fn protects_unsaved_and_later_edits(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote, 2);
        let root = fixture.chat.directory.path().join("project");
        let (shell, visual) = fixture.mount(cx);
        tap(visual, "live-chat-input");
        visual.simulate_input("Unsent conversation 中文 🙂");
        let source = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
        let editor = open_file(visual, &shell);
        visual.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_value("Unsaved file 中文 🙂", window, cx)
            })
        });
        wait(visual, |cx| {
            fixture::documents(&shell, cx).read(cx).has_unsaved(cx)
        });
        let state = restore(visual, &fixture, &shell);
        start(visual, &state, Some("file-0.txt"));
        assert_eq!(
            state.read_with(visual, |state, _| state.issue),
            Some("checkpoint_unsaved")
        );
        wait(visual, |_| true);
        assert!(!visual.update(|window, cx| window.notifications(cx).is_empty()));
        assert!(visual.debug_bounds("turn-changes-retry").is_some());
        assert_eq!(
            std::fs::read_to_string(root.join("file-0.txt")).unwrap(),
            after(0)
        );
        visual.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_value(after(0), window, cx);
                editor.set_selected_range(1..4, cx);
            })
        });
        wait(visual, |cx| {
            !fixture::documents(&shell, cx).read(cx).has_unsaved(cx)
        });
        start(visual, &state, Some("file-0.txt"));
        assert!(state.read_with(visual, |state, _| state.restored(Some("file-0.txt"))));
        assert_eq!(
            std::fs::read_to_string(root.join("file-0.txt")).unwrap(),
            before(0)
        );
        assert_eq!(
            std::fs::read_to_string(root.join("file-1.txt")).unwrap(),
            after(1)
        );
        let same = open_file(visual, &shell);
        wait(visual, |cx| same.read(cx).value().as_ref() == before(0));
        assert_eq!(same.entity_id(), editor.entity_id());
        assert_eq!(
            editor.read_with(visual, |editor, _| editor.selected_range()),
            1..4
        );
        std::fs::write(root.join("file-1.txt"), "Later edit").unwrap();
        start(visual, &state, Some("file-1.txt"));
        assert_eq!(
            state.read_with(visual, |state, _| state.issue),
            Some("checkpoint_conflict")
        );
        assert_eq!(
            std::fs::read_to_string(root.join("file-1.txt")).unwrap(),
            "Later edit"
        );
        visual.update(|window, cx| source.update(cx, |source, cx| source.focus(window, cx)));
        assert_eq!(copied(visual), "Unsent conversation 中文 🙂");
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(
            fixture
                .chat
                .runtime
                .block_on(fixture.chat.binding.client.read_conversation(
                    fixture.chat.session.id,
                    None,
                    100
                ))
                .unwrap()
                .page,
            fixture.original
        );
        fixture.close();
    }
}

#[gpui::test]
fn reverses_repeated_writes(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::writes(remote, 1, "new.txt", true);
        let root = fixture.chat.directory.path().join("project");
        let (shell, visual) = fixture.mount(cx);
        let state = restore(visual, &fixture, &shell);
        assert_eq!(
            std::fs::read_to_string(root.join("file-0.txt")).unwrap(),
            "Second write"
        );
        start(visual, &state, Some("file-0.txt"));
        assert!(state.read_with(visual, |state, _| state.restored(Some("file-0.txt"))));
        assert_eq!(
            std::fs::read_to_string(root.join("file-0.txt")).unwrap(),
            before(0)
        );
        assert_eq!(
            std::fs::read_to_string(root.join("new.txt")).unwrap(),
            "Created 中文 🙂"
        );
        fixture.close();
    }
}

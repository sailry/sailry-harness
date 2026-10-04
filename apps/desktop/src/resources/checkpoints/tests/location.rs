use super::*;
use crate::{conversation::live::Event, resources::SideResource};

#[gpui::test]
fn retains_original_location(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote, 1);
        let root = fixture.chat.directory.path().join("project");
        let repository = git2::Repository::init(&root).unwrap();
        let mut index = repository.index().unwrap();
        index
            .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree = index.write_tree().unwrap();
        let signature = git2::Signature::now("Fixture", "fixture@example.test").unwrap();
        repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "Snapshot",
                &repository.find_tree(tree).unwrap(),
                &[],
            )
            .unwrap();
        let original = fixture.chat.session.worktree;
        let Output::GitStatus(status) = fixture
            .chat
            .execute(Command::InspectGit { worktree: original })
        else {
            panic!("status expected")
        };
        let Output::Worktree(tree) = fixture.chat.execute(Command::CreateManagedWorktree {
            project: fixture.chat.session.project.unwrap(),
            source: original,
            branch: "isolated".into(),
            expected_head: status.head.unwrap(),
            expected_index: status.index_revision.unwrap(),
            include_changes: false,
        }) else {
            panic!("worktree expected")
        };
        let (shell, visual) = fixture.mount(cx);
        let source = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
        source.update(visual, |_, cx| {
            cx.emit(Event::FileAt(original, "file-0.txt".into()))
        });
        wait(visual, |cx| match &shell.read(cx).side_resource {
            Some(SideResource::Plugin(panel)) => panel
                .read(cx)
                .documents
                .as_ref()
                .is_some_and(|documents| documents.read(cx).editor("file-0.txt").is_some()),
            _ => false,
        });
        let editor = visual.update(|_, cx| {
            fixture::documents(&shell, cx)
                .read(cx)
                .editor("file-0.txt")
                .unwrap()
        });
        visual.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_value("Unsaved history file", window, cx)
            })
        });
        fixture.chat.execute(Command::MoveConversation {
            session: fixture.chat.session.id,
            worktree: tree.id,
            expected_revision: 2,
        });
        wait(visual, |cx| {
            source.read(cx).binding().worktree == Some(tree.id)
        });
        assert!(visual.update(|_, cx| {
            let documents = fixture::documents(&shell, cx);
            documents.read(cx).scope() == (fixture.chat.node.id(), original)
                && documents.read(cx).has_unsaved(cx)
        }));
        assert_eq!(
            editor.read_with(visual, |editor, _| editor.value()),
            "Unsaved history file"
        );
        let restore = restore(visual, &fixture, &shell);
        start(visual, &restore, Some("file-0.txt"));
        assert_eq!(
            restore.read_with(visual, |state, _| state.issue),
            Some("checkpoint_unsaved")
        );
        visual.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.set_value(after(0), window, cx))
        });
        start(visual, &restore, Some("file-0.txt"));
        assert!(restore.read_with(visual, |state, _| state.restored(Some("file-0.txt"))));
        assert_eq!(
            std::fs::read_to_string(root.join("file-0.txt")).unwrap(),
            before(0)
        );
        assert_eq!(
            std::fs::read_to_string(std::path::Path::new(&tree.path).join("file-0.txt")).unwrap(),
            after(0)
        );
        source.update(visual, |_, cx| {
            cx.emit(Event::GitFile(original, Some("file-0.txt".into())))
        });
        wait(
            visual,
            |cx| matches!(&shell.read(cx).side_resource, Some(SideResource::Plugin(panel)) if panel.read(cx).resource_scope(cx) == Some((fixture.chat.node.id(), original)) && panel.read(cx).resource_active()),
        );
        let Output::Session(fork) = fixture.chat.execute(Command::ForkConversationAt {
            session: fixture.chat.session.id,
            worktree: original,
            expected_revision: 3,
        }) else {
            panic!("fork expected")
        };
        source.update(visual, |_, cx| {
            cx.emit(Event::Forked(Box::new(fork.clone())))
        });
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|chat| {
                chat.read(cx).session() == Some(fork.id)
                    && chat.read(cx).binding().worktree == Some(original)
            })
        });
        assert_ne!(fork.id, fixture.chat.session.id);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

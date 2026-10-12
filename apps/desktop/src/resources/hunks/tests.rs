use super::*;
use crate::resources::checkpoints::tests::fixture::{
    self, Fixture, after, before, init, open_file, wait,
};
use core::prelude::v1::test;
use gpui_kit::component::input::EditorState;
use gpui_kit::test::TestWindowExt as _;
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

mod lifecycle;

fn versioned(remote: bool, count: usize) -> Fixture {
    let fixture = Fixture::new(remote, count);
    let root = fixture.chat.directory.path().join("project");
    for index in 0..count {
        std::fs::write(root.join(format!("file-{index}.txt")), before(index)).unwrap();
    }
    let repository = git2::Repository::init(&root).unwrap();
    let mut index = repository.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    repository
        .commit(Some("HEAD"), &signature, &signature, "Initial", &tree, &[])
        .unwrap();
    for index in 0..count {
        std::fs::write(root.join(format!("file-{index}.txt")), after(index)).unwrap();
    }
    fixture
}

fn control(window: &mut Window, selector: &str) -> Option<(ElementId, SharedString)> {
    let suffix = selector.strip_prefix("turn-hunks-")?;
    let (action, index) = suffix.rsplit_once('-')?;
    let index: usize = index.parse().ok()?;
    let matches = window
        .find_all(SharedString::from(action.to_owned()))
        .into_iter()
        .filter(|target| {
            target
                .path()
                .iter()
                .any(|id| id.to_string().starts_with("turn-hunk-"))
        })
        .collect::<Vec<_>>();
    let target = matches.get(index)?;
    if !target.visible() {
        return None;
    }
    let parent = target
        .path()
        .iter()
        .find(|id| id.to_string().starts_with("turn-hunk-"))?
        .clone();
    Some((parent, action.to_owned().into()))
}
fn tap(cx: &mut VisualTestContext, selector: &str) {
    if selector.starts_with("turn-hunks-") {
        if cx.update(|window, cx| window.has_active_dialog(cx)) {
            std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
        }
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            if let Some((parent, action)) = control(window, selector) {
                window.within(parent).click(action, cx);
            } else {
                window.click(SharedString::from(selector.to_owned()), cx);
            }
        });
        wait(cx, |_| true);
    } else {
        fixture::tap(cx, selector);
    }
}

fn element(cx: &mut VisualTestContext, selector: &str) {
    let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        wait(cx, |_| true);
        let present = if selector.starts_with("turn-hunks-") {
            cx.update(|window, cx| {
                window.render_frame(cx);
                control(window, selector).is_some()
                    || window
                        .try_find(SharedString::from(selector.to_owned()))
                        .is_some_and(|target| target.visible())
            })
        } else {
            cx.debug_bounds(selector).is_some()
        };
        if present {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "hunk UI deadline: selector={selector:?}; observed={:?}",
            cx.update(|window, _| {
                use gpui_kit::test::TestQueryExt as _;
                window
                    .elements()
                    .into_iter()
                    .filter(|target| {
                        target
                            .path()
                            .iter()
                            .any(|id| id.to_string().starts_with("turn-hunk-"))
                    })
                    .collect::<Vec<_>>()
            }),
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn missing(cx: &mut VisualTestContext, selector: &str) {
    let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        wait(cx, |_| true);
        if cx.update(|window, cx| {
            window.render_frame(cx);
            window
                .try_find(SharedString::from(selector.to_owned()))
                .is_none()
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "hunk retry deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn open(cx: &mut VisualTestContext, fixture: &Fixture, index: usize) {
    let selector = format!("live-turn-hunks-{}-{index}", fixture.turn);
    element(cx, &selector);
    tap(cx, &selector);
    element(cx, "turn-hunks-accept-0");
}

fn close(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.close_dialog(cx));
}

fn indexed(fixture: &Fixture, path: &str) -> String {
    let repository = git2::Repository::open(fixture.chat.directory.path().join("project")).unwrap();
    let entry = repository
        .index()
        .unwrap()
        .get_path(Path::new(path), 0)
        .unwrap();
    String::from_utf8(repository.find_blob(entry.id).unwrap().content().to_vec()).unwrap()
}

fn observed(inner: Arc<dyn sailry_link::Transport>) -> Arc<crate::live::FileTransport> {
    Arc::new(crate::live::FileTransport {
        inner,
        requests: Mutex::new(Vec::new()),
        hold: AtomicBool::new(false),
        drop_next: AtomicBool::new(false),
        ready: Arc::new(tokio::sync::Semaphore::new(0)),
    })
}

#[gpui::test]
#[cfg(unix)]
fn uses_controls_without_changing_other_files(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = versioned(remote, 3);
        let root = fixture.chat.directory.path().join("project");
        let (shell, visual) = fixture.mount(cx);
        tap(visual, "live-chat-input");
        visual.simulate_input("Unsent conversation");
        let index = std::fs::read(root.join(".git/index")).unwrap();
        open(visual, &fixture, 0);
        tap(visual, "turn-hunks-accept-0");
        assert_eq!(
            std::fs::read_to_string(root.join("file-0.txt")).unwrap(),
            after(0)
        );
        assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
        close(visual);
        open(visual, &fixture, 0);
        tap(visual, "turn-hunks-reject-0");
        assert_eq!(
            std::fs::read_to_string(root.join("file-0.txt")).unwrap(),
            after(0)
        );
        close(visual);
        open(visual, &fixture, 1);
        tap(visual, "turn-hunks-stage-0");
        wait(visual, |_| indexed(&fixture, "file-1.txt") == after(1));
        assert_eq!(indexed(&fixture, "file-0.txt"), before(0));
        assert_eq!(indexed(&fixture, "file-2.txt"), before(2));
        let index = std::fs::read(root.join(".git/index")).unwrap();
        close(visual);
        open(visual, &fixture, 2);
        tap(visual, "turn-hunks-reject-0");
        wait(visual, |_| {
            std::fs::read_to_string(root.join("file-2.txt")).unwrap() == before(2)
        });
        assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
        assert_eq!(
            std::fs::read_to_string(root.join("file-0.txt")).unwrap(),
            after(0)
        );
        assert_eq!(
            std::fs::read_to_string(root.join("file-1.txt")).unwrap(),
            after(1)
        );
        close(visual);
        let source = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
        visual.update(|window, cx| source.update(cx, |source, cx| source.focus(window, cx)));
        assert_eq!(fixture::copied(visual), "Unsent conversation");
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
fn supports_review_without_a_repository(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote, 2);
        let root = fixture.chat.directory.path().join("project");
        let (_, visual) = fixture.mount(cx);
        open(visual, &fixture, 0);
        tap(visual, "turn-hunks-stage-0");
        assert!(!root.join(".git").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("file-0.txt")).unwrap(),
            after(0)
        );
        tap(visual, "turn-hunks-accept-0");
        close(visual);
        open(visual, &fixture, 1);
        tap(visual, "turn-hunks-reject-0");
        wait(visual, |_| {
            std::fs::read_to_string(root.join("file-1.txt")).unwrap() == before(1)
        });
        assert_eq!(
            std::fs::read_to_string(root.join("file-0.txt")).unwrap(),
            after(0)
        );
        close(visual);
        fixture.close();
    }
}

#[gpui::test]
fn protects_drafts_and_conflicting_context(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote, 2);
        let root = fixture.chat.directory.path().join("project");
        let (shell, visual) = fixture.mount(cx);
        let editor: Entity<EditorState> = open_file(visual, &shell);
        visual.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_value("Unsaved draft", window, cx)
            })
        });
        wait(visual, |cx| {
            fixture::documents(&shell, cx).read(cx).has_unsaved(cx)
        });
        open(visual, &fixture, 0);
        tap(visual, "turn-hunks-reject-0");
        assert_eq!(
            std::fs::read_to_string(root.join("file-0.txt")).unwrap(),
            after(0)
        );
        assert_eq!(
            editor.read_with(visual, |editor, _| editor.value()),
            "Unsaved draft"
        );
        assert!(!visual.update(|window, cx| window.notifications(cx).is_empty()));
        close(visual);
        visual.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.set_value(after(0), window, cx))
        });
        wait(visual, |cx| {
            !fixture::documents(&shell, cx).read(cx).has_unsaved(cx)
        });
        open(visual, &fixture, 0);
        tap(visual, "turn-hunks-reject-0");
        wait(visual, |_| {
            std::fs::read_to_string(root.join("file-0.txt")).unwrap() == before(0)
        });
        wait(visual, |cx| editor.read(cx).value().as_ref() == before(0));
        close(visual);
        open(visual, &fixture, 1);
        std::fs::write(root.join("file-1.txt"), "Later manual change\n").unwrap();
        tap(visual, "turn-hunks-reject-0");
        element(visual, "turn-hunks-retry");
        assert_eq!(
            std::fs::read_to_string(root.join("file-1.txt")).unwrap(),
            "Later manual change\n"
        );
        std::fs::write(root.join("file-1.txt"), after(1)).unwrap();
        tap(visual, "turn-hunks-retry");
        missing(visual, "turn-hunks-retry");
        tap(visual, "turn-hunks-reject-0");
        wait(visual, |_| {
            std::fs::read_to_string(root.join("file-1.txt")).unwrap() == before(1)
        });
        close(visual);
        fixture.close();
    }
}

#[gpui::test]
#[cfg(unix)]
fn retains_the_original_worktree(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = versioned(remote, 1);
        let root = fixture.chat.directory.path().join("project");
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
            branch: "review-location".into(),
            expected_head: status.head.unwrap(),
            expected_index: status.index_revision.unwrap(),
            include_changes: false,
        }) else {
            panic!("worktree expected")
        };
        let replacement = "Manual data in the session's new worktree\n";
        std::fs::write(Path::new(&tree.path).join("file-0.txt"), replacement).unwrap();
        let (shell, visual) = fixture.mount(cx);
        let source = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
        fixture.chat.execute(Command::MoveConversation {
            session: fixture.chat.session.id,
            worktree: tree.id,
            expected_revision: fixture.chat.session.revision,
        });
        wait(visual, |cx| {
            source.read(cx).binding().worktree == Some(tree.id)
        });
        open(visual, &fixture, 0);
        tap(visual, "turn-hunks-reject-0");
        wait(visual, |_| {
            std::fs::read_to_string(root.join("file-0.txt")).unwrap() == before(0)
        });
        assert_eq!(
            std::fs::read_to_string(Path::new(&tree.path).join("file-0.txt")).unwrap(),
            replacement
        );
        assert_eq!(indexed(&fixture, "file-0.txt"), before(0));
        close(visual);
        fixture.close();
    }
}

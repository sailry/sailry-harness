use super::*;
use core::prelude::v1::test;
use gpui_kit::ElementId;
use gpui_kit::component::diff::{DiffLineRange, DiffSide};
use gpui_kit::test::{ElementSnapshot, TestAppContextExt as _, TestWindowExt as _};
use std::time::Duration;

fn source_rows(window: &Window) -> Vec<ElementSnapshot> {
    let id = ElementId::from("source");
    gpui_kit::base::test_support::snapshots(window)
        .into_iter()
        .filter(|snapshot| snapshot.path().last() == Some(&id))
        .collect()
}

#[test]
fn parses_captured_hunks_and_new_files() {
    let path = "nested/一 two.txt";
    let files = crate::diff::files("@@ -30,2 +40,2 @@\n same\n-old\n+new\n", path, false).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path().as_ref(), path);
    assert_eq!((files[0].additions(), files[0].deletions()), (1, 1));
    for source in ["", "一\ntwo 🙂\n", "one\ntwo", "--- content\n+++ content\n"] {
        let files = crate::diff::files(source, path, true).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path().as_ref(), path);
        assert_eq!(files[0].deletions(), 0);
        assert_eq!(files[0].additions(), source.lines().count());
    }
}

#[test]
fn preserves_binary_and_extended_headers() {
    let files = crate::diff::files("diff --git a/image.png b/image.png\nnew file mode 100644\nindex 0000000..1234567\nBinary files /dev/null and b/image.png differ\n", "ignored", false).unwrap();
    assert_eq!(files[0].path().as_ref(), "image.png");
    assert!(files[0].is_binary());
    assert!(
        files[0]
            .extended_headers()
            .iter()
            .any(|line| line.as_ref() == "new file mode 100644")
    );
    let text = "@@ -0,0 +1,2 @@\n+Binary files a/file and b/file differ\n+GIT binary patch\n";
    assert!(!crate::diff::files(text, "text.txt", false).unwrap()[0].is_binary());
}

#[test]
fn retains_available_truncated_rows() {
    let files = crate::diff::files(
        "@@ -7,20 +8,20 @@\n-old\n+new\n same\n",
        "partial.txt",
        false,
    )
    .unwrap();
    assert_eq!((files[0].additions(), files[0].deletions()), (1, 1));
    assert!(crate::diff::files("@@ invalid @@\n+one\n", "bad.txt", false).is_err());
    assert!(crate::diff::files("@@ -1 +1 @@\n-one\n-two\n+new\n", "bad.txt", false).is_err());
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::tests::init(cx);
    });
}
fn open(cx: &mut TestAppContext, text: &str, added: bool) -> WindowHandle<State> {
    let source = text.to_owned();
    cx.open_window(size(px(360.), px(180.)), move |_, cx| {
        let mut state = State::new(cx);
        state.set_patch(&source, "fixture.txt", added, cx);
        state
    })
}
async fn ready(cx: &mut TestAppContext, handle: WindowHandle<State>) {
    cx.wait_for(handle.into(), Duration::from_secs(10), |window, cx| {
        let ready = window
            .root::<State>()
            .flatten()
            .unwrap()
            .read(cx)
            .parse
            .is_none();
        ready && source_rows(window).iter().any(|line| line.visible())
    })
    .await;
}

#[gpui_kit::test]
async fn copies_large_patches(cx: &mut TestAppContext) {
    init(cx);
    let source = (0..100_000)
        .map(|index| format!("line {index}\n"))
        .collect::<String>();
    let handle = open(cx, &source, true);
    ready(cx, handle).await;
    cx.update_window(handle.into(), |_, window, cx| {
        window.within(("new", 0usize)).click(("line", 0usize), cx);
        window.press("secondary-a", cx);
        window.press("secondary-c", cx);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some(source.as_str())
        );
    })
    .unwrap();
}

#[gpui_kit::test]
async fn identical_patch_keeps_selection_and_state(cx: &mut TestAppContext) {
    init(cx);
    let text = "@@ -1 +1 @@\n-old\n+new 🙂\n";
    let handle = open(cx, text, false);
    ready(cx, handle).await;
    cx.update_window(handle.into(), |_, window, cx| {
        let state = window.root::<State>().flatten().unwrap();
        let diff = state.read(cx).diff.clone();
        let entity = diff.entity_id();
        window.within(("new", 0usize)).click(("line", 0usize), cx);
        let selection = diff.read(cx).selected_lines();
        assert_eq!(
            selection,
            Some(DiffLineRange::new("fixture.txt", DiffSide::Modified, 1, 1))
        );
        state.update(cx, |state, cx| {
            let generation = state.generation;
            state.set_patch(text, "fixture.txt", false, cx);
            assert_eq!(state.generation, generation);
            assert_eq!(state.diff.entity_id(), entity);
            assert_eq!(state.diff.read(cx).selected_lines(), selection);
        });
        window.press("secondary-c", cx);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("new 🙂\n")
        );
    })
    .unwrap();
}

#[gpui_kit::test]
async fn newer_patch_wins_background_completion(cx: &mut TestAppContext) {
    init(cx);
    let first = (0..100_000)
        .map(|index| format!("old {index}\n"))
        .collect::<String>();
    let handle = open(cx, &first, true);
    handle
        .update(cx, |state, _, cx| {
            state.set_patch("最新\n", "latest.txt", true, cx)
        })
        .unwrap();
    ready(cx, handle).await;
    cx.update_window(handle.into(), |_, window, cx| {
        let state = window.root::<State>().flatten().unwrap();
        assert_eq!(state.read(cx).generation, 2);
        let diff = state.read(cx).diff.clone();
        assert_eq!(diff.read(cx).files()[0].path().as_ref(), "latest.txt");
        window.within(("new", 0usize)).click(("line", 0usize), cx);
        window.press("secondary-a", cx);
        window.press("secondary-c", cx);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some("最新\n")
        );
        assert_eq!(state.read(cx).summary(cx), (1, 0, false));
    })
    .unwrap();
    cx.run_until_parked();
    handle
        .update(cx, |state, _, cx| {
            assert_eq!(state.diff.read(cx).files()[0].path().as_ref(), "latest.txt");
        })
        .unwrap();
}

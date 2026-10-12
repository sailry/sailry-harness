use super::*;
use core::prelude::v1::test;
use gpui_kit::component::diff::{DiffLinePosition, DiffLineRange, DiffSide};
use gpui_kit::test::{TestAppContextExt as _, TestWindowExt as _};
use std::time::Duration;

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
        ready && window.find_all("source").iter().any(|line| line.visible())
    })
    .await;
}

#[gpui_kit::test]
async fn virtualizes_and_copies_large_patches(cx: &mut TestAppContext) {
    init(cx);
    let source = (0..100_000)
        .map(|index| format!("line {index}\n"))
        .collect::<String>();
    let handle = open(cx, &source, true);
    ready(cx, handle).await;
    cx.update_window(handle.into(), |_, window, cx| {
        let rows = window.find_all("source");
        assert!(!rows.is_empty() && rows.len() < 40);
        window.within(("new", 0usize)).click(("line", 0usize), cx);
        window.press("secondary-a", cx);
        window.press("secondary-c", cx);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some(source.as_str())
        );
        let state = window.root::<State>().flatten().unwrap();
        let diff = state.read(cx).diff.clone();
        diff.update(cx, |state, cx| {
            state.scroll_to_line(
                DiffLinePosition::new("fixture.txt", DiffSide::Modified, 100_000),
                cx,
            )
        });
    })
    .unwrap();
    cx.wait_for(handle.into(), Duration::from_secs(10), |window, _| {
        window
            .find_all("source")
            .iter()
            .any(|line| line.label() == Some("line 99999"))
    })
    .await;
    handle
        .update(cx, |_, window, _| {
            assert!(window.find_all("source").len() < 40);
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

#[gpui_kit::test]
async fn refreshes_hints_without_resetting_views(cx: &mut TestAppContext) {
    struct Hints {
        list: ListState,
        focus: Vec<FocusHandle>,
    }
    impl Render for Hints {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let focus = self.focus.clone();
            list(self.list.clone(), move |index, _, _| {
                div()
                    .id(("hint-row", index))
                    .test_support()
                    .track_focus(&focus[index])
                    .w_full()
                    .h(px(if index == 0 { 40. } else { 20. }))
                    .into_any_element()
            })
            .size_full()
        }
    }
    init(cx);
    let handle = cx.open_window(size(px(200.), px(120.)), |_, cx| {
        let focus = (0..100).map(|_| cx.focus_handle()).collect::<Vec<_>>();
        let list = ListState::new(100, ListAlignment::Top, px(0.));
        list.splice_focusable(0..100, focus.iter().cloned().map(Some));
        let list = list.with_uniform_item_height(px(20.));
        Hints { list, focus }
    });
    cx.wait_for(handle.into(), Duration::from_secs(10), |window, _| {
        window.try_find(("hint-row", 0usize)).is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        let root = window.root::<Hints>().flatten().unwrap();
        let list = root.read(cx).list.clone();
        let focus = root.read(cx).focus[90].clone();
        // The first layout clears estimates when it learns the width.
        let _ = list.clone().with_uniform_item_height(px(20.));
        window.render_frame(cx);
        window.focus(&focus, cx);
        window.render_frame(cx);
        assert_eq!(window.find(("hint-row", 90usize)).focused(), Some(true));
        let row = window.find(("hint-row", 0usize)).bounds();
        assert_eq!(row.size.height, px(40.));
        let viewport = list.viewport_bounds();
        let offset = list.logical_scroll_top();
        let maximum = list.max_offset_for_scrollbar();
        let measured = list.bounds_for_item(0).unwrap();
        let _ = list.clone().with_uniform_item_height(px(30.));
        assert_eq!(list.bounds_for_item(0), Some(measured));
        assert_eq!(list.item_count(), 100);
        assert!(list.max_offset_for_scrollbar().y > maximum.y);
        assert_eq!(list.logical_scroll_top().item_ix, offset.item_ix);
        assert_eq!(
            list.logical_scroll_top().offset_in_item,
            offset.offset_in_item
        );
        window.render_frame(cx);
        assert_eq!(list.viewport_bounds(), viewport);
        assert_eq!(window.find(("hint-row", 0usize)).bounds(), row);
        assert_eq!(window.find(("hint-row", 90usize)).focused(), Some(true));
        assert_eq!(list.logical_scroll_top().item_ix, offset.item_ix);
        assert_eq!(
            list.logical_scroll_top().offset_in_item,
            offset.offset_in_item
        );
    })
    .unwrap();
}

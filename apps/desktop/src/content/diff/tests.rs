use super::super::{additions, is_binary, unified};
use super::*;
use core::prelude::v1::test;

#[test]
fn preserves_source_coordinates() {
    let lines = unified(
        "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -30,2 +40,2 @@\n same\n-old\n+new\n@@ -55 +65 @@\n--- content\n+++ content\n\\ No newline at end of file\n",
    );
    let code = lines
        .iter()
        .filter(|line| line.old.is_some() || line.new.is_some())
        .collect::<Vec<_>>();
    assert_eq!(
        code.iter()
            .map(|line| (line.old, line.new))
            .collect::<Vec<_>>(),
        [
            (Some(30), Some(40)),
            (Some(31), None),
            (None, Some(41)),
            (Some(55), None),
            (None, Some(65))
        ]
    );
    assert_eq!(
        code.iter().map(|line| line.text).collect::<Vec<_>>(),
        ["same", "old", "new", "-- content", "++ content"]
    );
    assert_eq!(lines.last().unwrap().kind, Kind::Meta);
}

#[test]
fn empty_sides() {
    let added = unified("@@ -0,0 +1,2 @@\n+{\n+}\n");
    assert_eq!((added[1].old, added[1].new), (None, Some(1)));
    assert_eq!((added[2].old, added[2].new), (None, Some(2)));
    let removed = unified("@@ -1,2 +0,0 @@\n-{\n-}\n");
    assert_eq!((removed[1].old, removed[1].new), (Some(1), None));
    assert_eq!((removed[2].old, removed[2].new), (Some(2), None));
    let created = additions("一\ntwo 🙂\n");
    assert!(
        created
            .iter()
            .all(|line| line.kind == Kind::Added && line.old.is_none())
    );
    assert_eq!(created[1].new, Some(2));
}

#[test]
fn rejects_inherited_coordinates() {
    let lines = unified(
        "@@ -1 +1 @@\n+one\ndiff --git a/b b/b\n+++ b/b\n@@ -7 +8 @@\n-before\n+after\n@@ invalid @@\n+metadata\n",
    );
    assert_eq!(lines[3].kind, Kind::Meta);
    assert_eq!((lines[5].old, lines[5].new), (Some(7), None));
    assert_eq!((lines[6].old, lines[6].new), (None, Some(8)));
    assert_eq!(lines[8].kind, Kind::Meta);
    assert_eq!((lines[8].old, lines[8].new), (None, None));
}

#[test]
fn recognizes_binary_metadata_without_hiding_text() {
    for text in [
        "diff --git a/image.png b/image.png\nnew file mode 100644\nindex 0000000..1234567\nBinary files /dev/null and b/image.png differ\n",
        "diff --git a/image.png b/image.png\nGIT binary patch\nliteral 3\nLcmaZq0001\n",
    ] {
        assert!(is_binary(&unified(text)));
    }
    for text in [
        "diff --git a/large.txt b/large.txt\nindex 1234567..abcdef0\n",
        "@@ -0,0 +1,2 @@\n+Binary files a/file and b/file differ\n+GIT binary patch\n",
        "diff --git a/image.png b/image.png\nBinary files a/image.png and b/image.png differ\ndiff --git a/text b/text\n@@ -1 +1 @@\n-old\n+new\n",
    ] {
        assert!(!is_binary(&unified(text)));
    }
}

#[test]
fn copies_unmounted_rows() {
    let source = (0..100_000)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let data = Data::new(&additions(&source));
    let selection = Selection {
        anchor: Cursor {
            row: 99_999,
            byte: 10,
        },
        cursor: Cursor { row: 0, byte: 0 },
    };
    assert_eq!(data.copied(selection), source);
}

#[test]
fn selection_boundaries() {
    let data = Data::new(&additions("a👩‍💻e\u{301}\n\nlast"));
    assert_eq!(data.clamp(Cursor { row: 0, byte: 5 }).byte, 1);
    let text = data.copied(Selection {
        anchor: Cursor { row: 0, byte: 1 },
        cursor: Cursor { row: 2, byte: 2 },
    });
    assert_eq!(text, "👩‍💻e\u{301}\n\nla");
    assert_eq!(selection::word("let value = 1", 6), 4..9);
}

#[test]
fn highlights_preserve_copy() {
    let data = Data::new(&unified("@@ -1 +1 @@\n-old\n+new"));
    assert_eq!(
        data.copied(Selection {
            anchor: Cursor { row: 1, byte: 0 },
            cursor: Cursor { row: 2, byte: 3 }
        }),
        "old\nnew"
    );
}

#[test]
fn selection_overlays_syntax() {
    let ink = rgb(0x112233).into();
    let selection = rgb(0x778899).into();
    let spans = selected_spans(
        vec![(
            0..3,
            HighlightStyle {
                color: Some(ink),
                ..Default::default()
            },
        )],
        Some(1..4),
        5,
        selection,
    );
    assert_eq!(
        spans
            .iter()
            .map(|(range, _)| range.clone())
            .collect::<Vec<_>>(),
        [0..1, 1..3, 3..4, 4..5]
    );
    assert_eq!(spans[1].1.color, Some(ink));
    assert_eq!(spans[1].1.background_color, Some(selection));
    assert_eq!(spans[3].1.background_color, None);
}

fn draw(handle: WindowHandle<State>, cx: &mut TestAppContext) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn virtualizes_large_files(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let source = (0..100_000)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let handle = cx.open_window(size(px(360.), px(180.)), |_, cx| {
        State::new(&additions(&source), "text", cx)
    });
    draw(handle, cx);
    handle
        .update(cx, |state, window, cx| {
            assert!(state.layouts.0.borrow().len() < 40);
            assert!(!state.layouts.0.borrow().is_empty());
            state.focus.focus(window, cx);
        })
        .unwrap();
    draw(handle, cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.dispatch_action(Box::new(SelectAll), cx)
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.dispatch_action(Box::new(Copy), cx)
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), source);
    handle
        .update(cx, |state, _, cx| {
            state.scroll.scroll_to_item(99_999, ScrollStrategy::Bottom);
            cx.notify();
        })
        .unwrap();
    draw(handle, cx);
    handle
        .update(cx, |state, _, cx| {
            {
                let layouts = state.layouts.0.borrow();
                assert!(layouts.len() < 40);
                assert!(layouts.keys().any(|row| *row >= 99_990));
            }
            state.selection = Some(Selection {
                anchor: Cursor { row: 0, byte: 0 },
                cursor: Cursor { row: 0, byte: 0 },
            });
            let scroll = state.scroll.0.borrow().base_handle.clone();
            let offset = scroll.offset();
            state.pointer = scroll.bounds().bottom_right() + point(px(30.), px(30.));
            state.dragging = true;
            state.autoscroll(cx);
            state.dragging = false;
            assert_eq!(scroll.offset(), offset);
            assert_eq!(state.selection.unwrap().cursor.row, 99_999);
        })
        .unwrap();
}

#[gpui_kit::test]
fn refresh_preserves_selection(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let handle = cx.open_window(size(px(360.), px(180.)), |_, cx| {
        State::new(&additions("one\ntwo"), "text", cx)
    });
    handle
        .update(cx, |state, _, cx| {
            state.selection = Some(Selection {
                anchor: Cursor { row: 0, byte: 0 },
                cursor: Cursor { row: 1, byte: 3 },
            });
            let scroll = state.scroll.0.clone();
            state.set_lines(&additions("one\ntwo\nthree"), cx);
            assert_eq!(state.data.copied(state.selection.unwrap()), "one\ntwo");
            assert!(std::rc::Rc::ptr_eq(&scroll, &state.scroll.0));
        })
        .unwrap();
}

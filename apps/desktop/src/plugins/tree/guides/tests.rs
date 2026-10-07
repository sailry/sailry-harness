use super::*;
use core::prelude::v1::test;

fn items() -> Vec<Item> {
    serde_json::from_value(json!([{
        "id": "root", "label": "Root", "expanded": true, "children": [
            {"id": "first", "label": "First", "expanded": true, "children": [
                {"id": "nested", "label": "Nested", "expanded": true, "children": [
                    {"id": "leaf-first", "label": "First leaf"},
                    {"id": "leaf-last", "label": "Last leaf"}
                ]},
                {"id": "tail", "label": "Tail"}
            ]},
            {"id": "last", "label": "Last", "expanded": true, "children": [
                {"id": "last-leaf", "label": "Last leaf"}
            ]}
        ]
    }]))
    .unwrap()
}

#[test]
fn retains_full_sibling_paths() {
    let items = items();
    let paths = paths(&items);
    assert_eq!(paths["root"], [false]);
    assert_eq!(paths["leaf-first"], [false, true, true, true]);
    assert_eq!(paths["leaf-last"], [false, true, true, false]);
    assert_eq!(paths["last-leaf"], [false, false, false]);
    let mut collapsed = items.clone();
    collapsed[0].expanded = false;
    assert_eq!(super::paths(&collapsed), paths);
}

#[test]
fn continues_ancestors_and_stops_last_siblings() {
    assert_eq!(
        columns(&[false, true, true, false], 0),
        [
            Column {
                index: 0,
                half: false,
                arm: false
            },
            Column {
                index: 1,
                half: false,
                arm: false
            },
            Column {
                index: 2,
                half: true,
                arm: true
            },
        ]
    );
    assert_eq!(
        columns(&[false, false, false], 0),
        [Column {
            index: 1,
            half: true,
            arm: true
        }]
    );
    assert_eq!(
        columns(&[false, true, true, true], 0).last(),
        Some(&Column {
            index: 2,
            half: false,
            arm: true
        })
    );
}

#[test]
fn skips_hidden_groups_and_flat_roots() {
    assert_eq!(
        columns(&[false, true, true, false], 1),
        [
            Column {
                index: 0,
                half: false,
                arm: false
            },
            Column {
                index: 1,
                half: true,
                arm: true
            },
        ]
    );
    assert!(columns(&[true], 0).is_empty());
    assert!(columns(&[false, false], 1).is_empty());
    assert!(columns(&[false, true], 4).is_empty());
}

struct Harness {
    tree: Entity<TreeState>,
    paths: BTreeMap<String, Vec<bool>>,
    indent: f32,
    skip: usize,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let paths = self.paths.clone();
        let indent = self.indent;
        let skip = self.skip;
        gpui_kit::base::Tree::new(&self.tree)
            .item(move |index, entry, _, _, cx| {
                let id = entry.item().id.to_string();
                let selector = format!("guide-row-{id}");
                crate::resources::rows::row(index, false, cx)
                    .debug_selector(move || selector.clone())
                    .relative()
                    .h_7()
                    .px_2()
                    .pl(px(8. + indent * entry.depth().saturating_sub(skip) as f32))
                    .children(super::render(&id, &paths[&id], skip, indent, cx))
                    .child(entry.item().label.clone())
                    .into_any_element()
            })
            .list_style(StyleRefinement::default().flex_grow_1().size_full())
            .size_full()
    }
}

#[gpui::test]
fn aligns_strokes_at_each_indent(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    for (indent, skip) in [(16., 0), (14., 1)] {
        let items = items();
        let paths = paths(&items);
        let tree = cx
            .new(|cx| TreeState::new(cx).items(items.iter().map(Item::native).collect::<Vec<_>>()));
        let (_, visual) = cx.add_window_view(|window, cx| {
            let harness = cx.new(|_| Harness {
                tree,
                paths,
                indent,
                skip,
            });
            Root::new(harness, window, cx)
        });
        let handle = visual.update(|window, _| window.window_handle());
        for width in [260., 320., 480.] {
            visual.simulate_window_resize(handle, size(px(width), px(400.)));
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let row = visual.debug_bounds("guide-row-leaf-last").unwrap();
            for index in 0..3 - skip {
                let selector = Box::leak(
                    format!("resource-guide-leaf-last-{index}-vertical").into_boxed_str(),
                );
                let line = visual.debug_bounds(selector).unwrap();
                assert_eq!(
                    line.left(),
                    row.left() + px(8. + indent * (index as f32 + 0.5))
                );
                assert_eq!(line.top(), row.top());
                assert_eq!(
                    line.size.height,
                    row.size.height * if index == 2 - skip { 0.5 } else { 1. }
                );
            }
            let selector =
                Box::leak(format!("resource-guide-leaf-last-{}-arm", 2 - skip).into_boxed_str());
            let arm = visual.debug_bounds(selector).unwrap();
            assert_eq!(arm.top(), row.top() + row.size.height * 0.5);
            assert_eq!(arm.size.width, px(indent * 0.5));
            assert_eq!(
                arm.right(),
                row.left() + px(8. + indent * (3 - skip) as f32)
            );
            assert!(
                visual
                    .debug_bounds("resource-guide-root-0-vertical")
                    .is_none()
            );
            if skip == 0 {
                assert!(
                    visual
                        .debug_bounds("resource-guide-last-leaf-0-vertical")
                        .is_none()
                );
            } else {
                assert!(
                    visual
                        .debug_bounds("resource-guide-first-0-vertical")
                        .is_none()
                );
            }
        }
        visual.update(|window, _| window.remove_window());
    }
}

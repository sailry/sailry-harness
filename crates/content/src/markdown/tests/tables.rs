use super::*;
use gpui_kit::component::{Theme, ThemeMode};

struct Fixture {
    document: Doc,
    layouts: BlockLayouts,
    width: f32,
}

impl Render for Fixture {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(self.width))
            .debug_selector(|| "table-document".into())
            .child(with_selection(
                &self.document,
                Editing {
                    layouts: Some(&self.layouts),
                    ..Default::default()
                },
                window,
                cx,
            ))
    }
}

#[gpui::test]
fn wraps_every_cell_without_horizontal_overflow(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::tests::init(cx);
    });
    let source = "| 方面 | Python | Rust | Go |\n| --- | :--- | :---: | ---: |\n| 内存管理 | 自动管理，开发者通常无需手动释放内存。CPython 使用引用计数和循环垃圾回收。 | Ownership keeps allocations scoped without a garbage collector and retains every word in a narrow cell. | Garbage collection manages allocations and avoids manual memory management. |\n| 并发模型 | 支持线程、多进程及 `asyncio` 异步编程。 | `averylongidentifierwithoutanyspacesandwithallcharacterspreserved` | [Reference](https://example.com) |\n";
    let owner = cx.new(|_| Fixture {
        document: parse(source),
        layouts: BlockLayouts::default(),
        width: 320.,
    });
    let fixture = owner.clone();
    let (_, visual) = cx.add_window_view(|window, cx| Root::new(fixture, window, cx));
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [320., 960.] {
            visual.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                owner.update(cx, |view, cx| {
                    view.width = width;
                    cx.notify();
                });
            });
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let bounds = visual.debug_bounds("table-document").unwrap();
            assert_eq!(bounds.size.width, px(width));
            let columns = (0..4)
                .map(|column| {
                    visual
                        .debug_bounds(Box::leak(
                            format!("markdown-table-cell-0-0-{column}").into_boxed_str(),
                        ))
                        .unwrap()
                })
                .collect::<Vec<_>>();
            owner.read_with(visual, |fixture, _| {
                let BlockKind::Table { header, rows, .. } = &fixture.document.blocks[0].kind else {
                    panic!("the source must remain a table");
                };
                for (row, cells) in std::iter::once(header).chain(rows).enumerate() {
                    for (column, cell) in cells.iter().enumerate() {
                        let part = Part::Cell { row, column };
                        let (start, height) =
                            fixture.layouts.position(Cursor::new(0, part, 0)).unwrap();
                        let (end, _) = fixture
                            .layouts
                            .position(Cursor::new(0, part, cell.text.len()))
                            .unwrap();
                        assert!(start.x >= columns[column].left());
                        assert!(end.x <= columns[column].right());
                        if row == 1 && column > 0 {
                            assert!(end.y >= start.y + height);
                        }
                        for (offset, _) in cell.text.char_indices() {
                            let (position, _) = fixture
                                .layouts
                                .position(Cursor::new(0, part, offset))
                                .unwrap();
                            assert!(position.x >= columns[column].left());
                            assert!(position.x <= columns[column].right());
                        }
                    }
                }
                assert!(
                    selectable::copied(&fixture.document, Selection::all(&fixture.document))
                        .contains(
                            "averylongidentifierwithoutanyspacesandwithallcharacterspreserved"
                        )
                );
            });
            assert_eq!(columns.first().unwrap().left(), bounds.left());
            assert!((columns.last().unwrap().right() - bounds.right()).abs() < px(0.01));
            for row in 1..=2 {
                for (column, header) in columns.iter().enumerate() {
                    let cell = visual
                        .debug_bounds(Box::leak(
                            format!("markdown-table-cell-0-{row}-{column}").into_boxed_str(),
                        ))
                        .unwrap();
                    assert_eq!(cell.left(), header.left());
                    assert_eq!(cell.right(), header.right());
                    assert!(cell.size.height > px(24.));
                }
            }
        }
    }
}

use super::*;
use crate::plugins::tests::{click, init};
use core::prelude::v1::test;

struct Harness {
    table: Entity<TableState<Rows>>,
}
impl Render for Harness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        DataTable::new(&self.table)
            .with_size(Size::Medium)
            .when(
                self.table.read(cx).delegate().size() != Size::Medium,
                |table| table.with_size(self.table.read(cx).delegate().size()),
            )
            .bordered(false)
            .stripe(true)
            .empty_stripes(true)
    }
}

#[gpui::test]
fn viewport_fillers_preserve_counts_and_ignore_input(cx: &mut TestAppContext) {
    init(cx);
    for header in [false, true] {
        for values in [Vec::<Vec<String>>::new(), vec![vec!["First".into()]]] {
            let count = values.len();
            let props: Props = serde_json::from_value(json!({
                "revision":"1", "columns":["Name"], "rows":values,
                "stripe":true, "empty_stripes":true, "header":header,
                "menu":[{"id":"copy","label":"Copy"}],
            }))
            .unwrap();
            let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
            let mut owner = None;
            let (_, visual) = cx.add_window_view(|window, cx| {
                let events = cx.new(|_| Events::new(sender, CancellationToken::new()));
                let table = cx.new(|cx| {
                    TableState::new(
                        Rows {
                            id: "striped".into(),
                            props,
                            selected: BTreeSet::new(),
                            anchor: None,
                            cursor: None,
                            events,
                        },
                        window,
                        cx,
                    )
                });
                owner = Some(table.clone());
                let view = cx.new(|_| Harness { table });
                Root::new(view, window, cx)
            });
            let table = owner.unwrap();
            let handle = visual.update(|window, _| window.window_handle());
            for height in [240., 480.] {
                visual.simulate_window_resize(handle, size(px(400.), px(height)));
                // Native stripes settle after the list's viewport is measured.
                for _ in 0..4 {
                    visual.run_until_parked();
                    visual.update(|window, cx| window.draw(cx).clear(cx));
                }
                let filler = visual.debug_bounds("striped-filler-2").unwrap();
                assert_eq!(
                    filler.size.height,
                    gpui_kit::component::Size::Medium.table_row_height()
                );
                let header_height = visual.debug_bounds("striped-header").unwrap().size.height;
                assert_eq!(header_height, if header { px(32.) } else { px(0.) },);
                if count > 0 {
                    let row = visual.debug_bounds("striped-row-0").unwrap();
                    assert_eq!(row.size.height, px(32.));
                    assert_eq!(row.size.height, filler.size.height);
                }
                if header {
                    assert_eq!(header_height, filler.size.height);
                }
                assert!(visual.debug_bounds("striped-filler-5").is_some());
                click(visual, "striped-filler-2");
                visual.simulate_mouse_down(
                    filler.center(),
                    MouseButton::Right,
                    Modifiers::default(),
                );
                visual.simulate_mouse_up(filler.center(), MouseButton::Right, Modifiers::default());
                visual.run_until_parked();
                assert!(receiver.try_recv().is_err());
                table.read_with(visual, |table, cx| {
                    assert_eq!(table.delegate().rows_count(cx), count);
                    assert_eq!(table.dump(cx).1.len(), count);
                    assert!(table.delegate().selected.is_empty());
                    assert_eq!(table.selected_row(), None);
                    assert_eq!(table.right_clicked_row(), None);
                });
            }
            if count > 0 {
                click(visual, "striped-row-0");
                let event = receiver.try_recv().unwrap();
                assert_eq!(event["kind"], "selection");
                assert_eq!(event["row"], 0);
                assert_eq!(event["rows"], json!([0]));
                assert!(receiver.try_recv().is_err());
            }
            visual.update(|window, _| window.remove_window());
        }
    }
}

#[test]
fn stripes_and_headers_are_opt_in() {
    let props: Props = serde_json::from_value(json!({
        "revision":"1", "columns":[], "rows":[],
    }))
    .unwrap();
    assert!(!props.stripe);
    assert!(!props.empty_stripes);
    assert!(props.header);
    assert!(props.empty_icon.is_none());
}

#[gpui::test]
fn headers_and_cells_share_column_alignment(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let (sender, _) = tokio::sync::mpsc::channel(8);
        let events = cx.new(|_| Events::new(sender, CancellationToken::new()));
        let rows = Rows {
            id: "aligned".into(),
            props: serde_json::from_value(json!({
                "revision":"1", "columns":["Input", "Model", "Provider"], "rows":[],
                "alignments":["end", "start"],
            }))
            .unwrap(),
            selected: BTreeSet::new(),
            anchor: None,
            cursor: None,
            events,
        };
        // Header and body composition both start with this native styled cell.
        for (column, alignment) in [TextAlign::Right, TextAlign::Left, TextAlign::Left]
            .into_iter()
            .enumerate()
        {
            assert_eq!(rows.cell(column).text_style().text_align, Some(alignment));
        }
    });
}

#[gpui::test]
fn multiline_cells_preserve_selection_and_scrolling(cx: &mut TestAppContext) {
    init(cx);
    for height in [None, Some(48.)] {
        let rows: Vec<_> = (0..50)
            .map(|row| vec![format!("Exact input {row} · exact output {row}")])
            .collect();
        let cells: Vec<_> = (0..50).map(|row| vec![json!({
        "primary":{"text":format!("Input {row}"),"icon":"reicon:arrows/arrow-down","tone":"chart_2"},
        "secondary":{"text":format!("Output {row}"),"icon":"reicon:arrows/arrow-up","tone":"chart_4"},
    })]).collect();
        let props: Props = serde_json::from_value(json!({
            "revision":"1", "columns":["Input / Output"], "rows":rows,
            "cells":cells, "alignments":["end"], "widths":[240], "row_height":height,
        }))
        .unwrap();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let events = cx.new(|_| Events::new(sender, CancellationToken::new()));
            let table = cx.new(|cx| {
                TableState::new(
                    Rows {
                        id: "paired".into(),
                        props,
                        selected: BTreeSet::new(),
                        anchor: None,
                        cursor: None,
                        events,
                    },
                    window,
                    cx,
                )
            });
            owner = Some(table.clone());
            let view = cx.new(|_| Harness { table });
            Root::new(view, window, cx)
        });
        let table = owner.unwrap();
        visual.simulate_resize(size(px(340.), px(240.)));
        let settle = |visual: &mut VisualTestContext| {
            for _ in 0..4 {
                visual.run_until_parked();
                visual.update(|window, cx| window.draw(cx).clear(cx));
            }
        };
        settle(visual);
        let row = visual.debug_bounds("paired-row-0").unwrap();
        let primary = visual.debug_bounds("paired-cell-0-0-primary").unwrap();
        let secondary = visual.debug_bounds("paired-cell-0-0-secondary").unwrap();
        assert_eq!(row.size.height, px(height.unwrap_or(32.)));
        assert_eq!(
            visual.debug_bounds("paired-header").unwrap().size.height,
            row.size.height
        );
        let header = visual.debug_bounds("paired-header").unwrap();
        let label = visual.debug_bounds("paired-header-0").unwrap();
        assert!(label.top() >= header.top() && label.bottom() <= header.bottom());
        assert!((label.center().y - header.center().y).abs() <= px(1.));
        assert!(primary.top() >= row.top() && secondary.bottom() <= row.bottom());
        assert!(primary.bottom() <= secondary.top());
        click(visual, "paired-cell-0-0-secondary");
        assert_eq!(receiver.try_recv().unwrap()["rows"], json!([0]));
        let next = visual.debug_bounds("paired-row-1").unwrap();
        visual.simulate_mouse_down(
            next.center(),
            MouseButton::Left,
            Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        visual.simulate_mouse_up(
            next.center(),
            MouseButton::Left,
            Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        assert_eq!(receiver.try_recv().unwrap()["rows"], json!([0, 1]));
        table.read_with(visual, |table, cx| {
            assert_eq!(
                table.dump(cx).1[0],
                vec!["Exact input 0 · exact output 0".to_owned()]
            )
        });
        table.update(visual, |table, cx| table.scroll_to_row(49, cx));
        settle(visual);
        assert!(visual.debug_bounds("paired-cell-49-0-secondary").is_some());
        click(visual, "paired-cell-49-0-secondary");
        assert_eq!(receiver.try_recv().unwrap()["rows"], json!([49]));
        visual.update(|window, _| window.remove_window());
    }
}

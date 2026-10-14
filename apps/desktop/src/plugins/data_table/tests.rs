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
    }
}

#[test]
fn stripes_and_headers_are_opt_in() {
    let props: Props = serde_json::from_value(json!({
        "revision":"1", "columns":[], "rows":[],
    }))
    .unwrap();
    assert!(!props.stripe);
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
            width: px(0.),
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
fn fill_column_absorbs_remaining_width(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let (sender, _) = tokio::sync::mpsc::channel(8);
        let events = cx.new(|_| Events::new(sender, CancellationToken::new()));
        let mut rows = Rows {
            id: "filled".into(),
            props: serde_json::from_value(json!({
                "revision":"1", "columns":["Name", "Share", "Cost"], "rows":[],
                "widths":[100, 60, 80], "fill":1,
            }))
            .unwrap(),
            selected: BTreeSet::new(),
            anchor: None,
            cursor: None,
            events,
            width: px(0.),
        };
        // Before layout the fill column keeps its declared minimum.
        assert_eq!(rows.fill(1), px(60.));
        rows.width = px(500.);
        assert_eq!(rows.fill(0), px(100.));
        assert_eq!(rows.fill(1), px(319.));
        assert_eq!(rows.fill(2), px(80.));
        rows.width = px(200.);
        assert_eq!(rows.fill(1), px(60.));
    });
}

#[test]
fn fill_index_must_name_a_column() {
    let props: Props = serde_json::from_value(json!({
        "revision":"1", "columns":["Name"], "rows":[], "fill":0,
    }))
    .unwrap();
    assert_eq!(props.fill, Some(0));
    assert!(
        serde_json::from_value::<Props>(json!({
            "revision":"1", "columns":["Name"], "rows":[], "fill":"wide",
        }))
        .is_err()
    );
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
                        width: px(0.),
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

mod inline_parts {
    use super::*;
    use gpui_kit::test::TestWindowExt as _;

    fn line(meter: serde_json::Value) -> cells::Line {
        serde_json::from_value(json!({
            "text":"12.3k","badge":"G","note":"Provider","chip":"Project",
            "text_tone":"muted","meter":meter,
        }))
        .unwrap()
    }

    #[test]
    fn validates_meter_and_badge_bounds() {
        assert!(line(json!({"value":42,"label":"42%","tone":"warning"})).valid());
        assert!(!line(json!({"value":120,"label":"120%"})).valid());
        assert!(!line(json!({"value":42,"label":"42%","width":8})).valid());
        let mut wide = line(json!({"value":42,"label":"42%"}));
        wide.badge = Some("ABC".into());
        assert!(!wide.valid());
    }

    #[gpui::test]
    fn share_one_line(cx: &mut TestAppContext) {
        init(cx);
        let props: Props = serde_json::from_value(json!({
            "revision":"1", "columns":["Model", "Input"], "rows":[["Model · Provider", "12.3k · 42%"]],
            "cells":[[
                {"primary":{"text":"Model","badge":"P","note":"Provider","chip":"Project"}},
                {"primary":{"text":"12.3k","meter":{"value":42,"label":"42%","tone":"warning","icon":"reicon:ui/alert-triangle"}}},
            ]],
            "alignments":["start", "end"], "widths":[240, 160], "row_height":40, "header_height":32,
        }))
        .unwrap();
        let (sender, _) = tokio::sync::mpsc::channel(8);
        let (_, visual) = cx.add_window_view(|window, cx| {
            let events = cx.new(|_| Events::new(sender, CancellationToken::new()));
            let table = cx.new(|cx| {
                TableState::new(
                    Rows {
                        id: "parts".into(),
                        props,
                        selected: BTreeSet::new(),
                        anchor: None,
                        cursor: None,
                        events,
                        width: px(0.),
                    },
                    window,
                    cx,
                )
            });
            let view = cx.new(|_| Harness { table });
            Root::new(view, window, cx)
        });
        visual.simulate_resize(size(px(480.), px(200.)));
        for _ in 0..4 {
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
        }
        let row = visual.debug_bounds("parts-row-0").unwrap();
        let name = visual.debug_bounds("parts-cell-0-0-primary").unwrap();
        let meter = visual.update(|window, _| window.find("parts-cell-0-1-primary-meter").bounds());
        assert_eq!(row.size.height, px(40.));
        assert!(name.top() >= row.top() && name.bottom() <= row.bottom());
        assert!(meter.top() >= row.top() && meter.bottom() <= row.bottom());
        assert!((meter.center().y - name.center().y).abs() <= px(1.));
        assert_eq!(meter.size, size(px(36.), px(4.)));
        assert!(visual.debug_bounds("parts-cell-0-0-secondary").is_none());
        visual.update(|window, _| window.remove_window());
    }

    #[gpui::test]
    fn badge_icons_and_fill_meters(cx: &mut TestAppContext) {
        init(cx);
        let props: Props = serde_json::from_value(json!({
            "revision":"1", "columns":["Model", "Share"], "rows":[["Model", "42%"]],
            "cells":[[
                {"primary":{"text":"Model","badge":"P","badge_icon":"icons/providers/openai.svg"}},
                {"primary":{"text":"","meter":{"value":42,"label":"42%","fill":true}}},
            ]],
            "widths":[160, 120], "fill":1, "row_height":40,
        }))
        .unwrap();
        let (sender, _) = tokio::sync::mpsc::channel(8);
        let (_, visual) = cx.add_window_view(|window, cx| {
            let events = cx.new(|_| Events::new(sender, CancellationToken::new()));
            let table = cx.new(|cx| {
                TableState::new(
                    Rows {
                        id: "filled".into(),
                        props,
                        selected: BTreeSet::new(),
                        anchor: None,
                        cursor: None,
                        events,
                        // The production wrapper measures this after layout.
                        width: px(480.),
                    },
                    window,
                    cx,
                )
            });
            let view = cx.new(|_| Harness { table });
            Root::new(view, window, cx)
        });
        visual.simulate_resize(size(px(480.), px(200.)));
        for _ in 0..4 {
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
        }
        let share = visual.debug_bounds("filled-cell-0-1").unwrap();
        let meter =
            visual.update(|window, _| window.find("filled-cell-0-1-primary-meter").bounds());
        // The 319px column minus Kit cell padding.
        assert!(share.size.width > px(280.) && share.size.width <= px(319.));
        assert!(meter.size.width > px(200.) && meter.right() <= share.right());
        let model = visual.debug_bounds("filled-cell-0-0-primary").unwrap();
        assert!(model.size.width > px(0.));
        visual.update(|window, _| window.remove_window());
    }
}

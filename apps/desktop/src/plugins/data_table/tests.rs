use super::*;
use core::prelude::v1::test;

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

mod inline_parts {
    use super::*;

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
}

use super::{
    heatmap::{GAP, Heatmap, LABELS, MAX_PITCH},
    stacked::{MAX_BAR, StackedChart},
    *,
};
use core::prelude::v1::test;
use serde_json::json;

fn heatmap(count: usize) -> Heatmap {
    let rows = if count <= 31 { 1 } else { 7 };
    let columns = count.div_ceil(rows);
    let props = json!({
        "rows":rows,"columns":columns,
        "cells":(0..count).map(|index|json!({
            "column":index/rows,"row":index%rows,"intensity":1.,
            "label":format!("{}",index/28+1),"label_group":format!("{}",index/28),
            "tooltip":{"heading":"2024-01-01 00:00","title":"10.6k Token",
                "rows":(0..9).map(|model|json!({"label":format!("model-{model}"),"value":"1.1k"})).collect::<Vec<_>>()}
        })).collect::<Vec<_>>()
    });
    Heatmap::new("heatmap".into(), serde_json::from_value(props).unwrap()).unwrap()
}
fn stacked() -> StackedChart {
    let props = json!({
        "series":[{"id":"a","color":"chart_3"},{"id":"b","color":"chart_2"},{"id":"c","color":"chart_4"}],
        "maximum":15,"ticks":[{"value":0,"label":"0"},{"value":15,"label":"15"}],
        "buckets":[
            {"label":"01-01","values":[4,8,3],"tooltip":{"heading":"2024-01-01 · UTC","rows":[{"label":"A","value":"4"},{"label":"B","value":"8"},{"label":"C","value":"3"}],"footer":{"label":"Total","value":"15"}}},
            {"label":"01-02","values":null,"tooltip":{"heading":"2024-01-02 · UTC","rows":[{"label":"A","value":"Unknown"},{"label":"B","value":"Unknown"},{"label":"C","value":"Unknown"}],"footer":{"label":"Total","value":"Unknown"}}}
        ]
    });
    StackedChart::new("stacked".into(), serde_json::from_value(props).unwrap()).unwrap()
}

/// Frame height for the plot width, matching the frame's sizing sibling.
fn height(chart: &Heatmap, width: f32) -> f32 {
    let rows = chart.props.rows as f32;
    (width / chart.props.columns as f32).min(MAX_PITCH) * rows + LABELS
}

#[test]
fn fits_all_columns() {
    for count in [7, 30, 365] {
        let chart = heatmap(count);
        for width in [120., 400., 1000.] {
            let bounds = Bounds::new(
                point(px(0.), px(0.)),
                size(px(width), px(height(&chart, width))),
            );
            let pitch = chart.pitch(bounds);
            assert!(pitch > 0. && pitch <= MAX_PITCH);
            let cells = chart
                .props
                .cells
                .iter()
                .map(|cell| chart.cell(cell, bounds).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(cells.len(), count);
            assert_eq!(cells[0].origin, point(px(0.), px(0.)));
            for cell in &cells {
                assert_eq!(cell.size.width, cell.size.height);
                assert!(cell.right() <= bounds.size.width);
                assert!(cell.bottom() <= px(chart.props.rows as f32 * pitch));
            }
            let last = cells
                .iter()
                .fold(px(0.), |right, cell| right.max(cell.right()));
            if pitch < MAX_PITCH {
                // Scaled cells reach the right edge, leaving only the trailing gap.
                assert!((px(width) - last).as_f32() <= pitch * GAP + 0.01);
            }
        }
    }
}

#[test]
fn disjoint_stacks() {
    let chart = stacked();
    assert_eq!(
        chart
            .series
            .iter()
            .map(|series| (series.points[0].y0, series.points[0].y1))
            .collect::<Vec<_>>(),
        [(0., 4.), (4., 12.), (12., 15.)]
    );
    assert!(
        chart
            .series
            .iter()
            .all(|series| series.points[1].y0 == 0. && series.points[1].y1 == 0.)
    );
    assert_eq!(chart.props.buckets[1].tooltip.footer.value, "Unknown");
}

mod stacked_bars {
    use super::*;

    fn chart(buckets: Vec<Option<[f32; 3]>>) -> StackedChart {
        let props = json!({
            "series":[{"id":"a","color":"chart_3"},{"id":"b","color":"chart_2"},{"id":"c","color":"chart_4"}],
            "maximum":4,"ticks":[{"value":0,"label":"0"}],"height":200,
            "buckets":buckets.into_iter().enumerate().map(|(index, values)| {
                let label = if index % 2 == 0 { index.to_string() } else { String::new() };
                json!({
                "label":label,
                "values":values,
                "tooltip":{"heading":"","rows":[{"label":"A","value":""},{"label":"B","value":""},{"label":"C","value":""}],"footer":{"label":"","value":""}}
            })}).collect::<Vec<_>>()
        });
        StackedChart::new("bars".into(), serde_json::from_value(props).unwrap()).unwrap()
    }

    #[test]
    fn marks_only_known_empty_buckets() {
        let chart = chart(vec![Some([0., 0., 0.]), None, Some([1., 0., 3.])]);
        assert_eq!(chart.empty(), [0]);
        assert_eq!(chart.props.height, Some(200.));
    }

    #[test]
    fn caps_and_centers_sparse_bars() {
        let area = Bounds::new(point(px(48.), px(8.)), size(px(800.), px(160.)));
        let sparse = chart(vec![Some([1., 1., 1.]); 7]);
        let scale = sparse.scale(area);
        let (width, inset) = StackedChart::bar(&scale);
        assert_eq!(width, MAX_BAR);
        assert!((inset * 2. + width - scale.band_width()).abs() < 0.01);
        let dense = chart(vec![Some([1., 1., 1.]); 48]);
        let scale = dense.scale(area);
        assert_eq!(StackedChart::bar(&scale), (scale.band_width(), 0.));
    }

    #[test]
    fn rejects_unbounded_heights() {
        let mut props = json!({"series":[],"maximum":1,"ticks":[],"buckets":[],"height":40});
        let parse = |value: &serde_json::Value| {
            StackedChart::new(
                "bars".into(),
                serde_json::from_value(value.clone()).unwrap(),
            )
        };
        assert!(parse(&props).is_none());
        props["height"] = json!(200);
        assert!(parse(&props).is_some());
    }
}

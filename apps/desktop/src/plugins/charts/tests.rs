use super::{
    heatmap::{CELL, Heatmap, PITCH},
    stacked::StackedChart,
    *,
};
use core::prelude::v1::test;
use gpui_kit::component::plot::scale::Scale;
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

#[test]
fn recent_columns() {
    for count in [7, 30, 365] {
        let chart = heatmap(count);
        let mut prior_visible = 0;
        for width in [120., 400., 1000.] {
            let bounds = Bounds::new(point(px(0.), px(0.)), size(px(width), chart.height()));
            let visible = chart
                .props
                .cells
                .iter()
                .filter_map(|cell| chart.cell(cell, bounds))
                .collect::<Vec<_>>();
            assert!(visible.len() >= prior_visible);
            prior_visible = visible.len();
            for cell in &visible {
                assert_eq!(cell.size, size(px(CELL), px(CELL)));
                assert!(cell.right() <= bounds.size.width);
                assert!(cell.bottom() <= px(chart.props.rows as f32 * PITCH));
            }
            let last = chart
                .cell(chart.props.cells.last().unwrap(), bounds)
                .unwrap();
            assert_eq!(
                last.left(),
                px((chart.columns_at(px(width)) - 1) as f32 * PITCH)
            );
            assert_eq!(
                chart.cell(&chart.props.cells[0], bounds).is_none(),
                chart.props.columns > chart.columns_at(px(width))
            );
        }
        assert_eq!(prior_visible, count);
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

struct HeatmapView;
impl Render for HeatmapView {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let chart = heatmap(365);
        let height = chart.height();
        div().size_full().p_4().child(
            div()
                .mt((window.viewport_size().height - height - px(48.)).max(px(0.)))
                .w_full()
                .h(height)
                .debug_selector(|| "plot-bounds".into())
                .child(chart),
        )
    }
}
#[gpui::test]
fn heatmap_tooltip_bounds(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) =
        cx.add_window_view(|window, cx| Root::new(cx.new(|_| HeatmapView), window, cx));
    let chart = heatmap(365);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [400., 900.] {
            let handle = visual.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            visual.simulate_window_resize(handle, size(px(width), px(720.)));
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let plot = visual.debug_bounds("plot-bounds").unwrap();
            let first = chart
                .props
                .cells
                .iter()
                .position(|cell| chart.cell(cell, plot).is_some())
                .unwrap();
            for index in [first, chart.props.cells.len() - 1] {
                let cell = chart.cell(&chart.props.cells[index], plot).unwrap();
                visual.update(|_, cx| {
                    assert_eq!(
                        chart.tooltip_state(cell.center(), plot, cx).unwrap().index,
                        index
                    );
                    assert!(
                        chart
                            .tooltip_state(point(cell.right() + px(0.1), cell.center().y), plot, cx)
                            .is_none()
                    );
                });
                visual.simulate_mouse_move(plot.origin + cell.center(), None, Modifiers::default());
                for _ in 0..3 {
                    visual.run_until_parked();
                    visual.update(|window, cx| window.draw(cx).clear(cx));
                }
                let tooltip = visual.debug_bounds("heatmap-tooltip").unwrap();
                assert!(tooltip.left() >= px(12.) && tooltip.right() <= px(width - 12.));
                assert!(
                    tooltip.top() >= px(12.) && tooltip.bottom() <= px(708.),
                    "tooltip={tooltip:?}, plot={plot:?}, width={width}, index={index}"
                );
            }
            visual.update(|_, cx| {
                assert!(
                    chart
                        .tooltip_state(
                            point(px(1.), px(chart.props.rows as f32 * PITCH + 8.)),
                            plot,
                            cx
                        )
                        .is_none()
                );
                assert!(
                    chart
                        .tooltip_state(point(px(1.), plot.size.height + px(8.)), plot, cx)
                        .is_none()
                );
            });
        }
    }
}

struct StackedView;
impl Render for StackedView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_4().child(
            div()
                .debug_selector(|| "plot-bounds".into())
                .w_full()
                .h(px(148.))
                .child(stacked()),
        )
    }
}
#[gpui::test]
fn stacked_tooltip_bounds(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) =
        cx.add_window_view(|window, cx| Root::new(cx.new(|_| StackedView), window, cx));
    let chart = stacked();
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [360., 900.] {
            let handle = visual.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            visual.simulate_window_resize(handle, size(px(width), px(400.)));
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let bounds = visual.debug_bounds("plot-bounds").unwrap();
            let area = StackedChart::area(bounds);
            let scale = chart.scale(area);
            for index in 0..2 {
                let position = point(
                    area.left() + px(scale.tick(&index).unwrap() + scale.band_width() / 2.),
                    area.center().y,
                );
                visual.update(|_, cx| {
                    assert_eq!(
                        chart.tooltip_state(position, bounds, cx).unwrap().index,
                        index
                    );
                    assert!(
                        chart
                            .tooltip_state(point(px(0.), px(0.)), bounds, cx)
                            .is_none()
                    );
                });
                visual.simulate_mouse_move(bounds.origin + position, None, Modifiers::default());
                for _ in 0..3 {
                    visual.run_until_parked();
                    visual.update(|window, cx| window.draw(cx).clear(cx));
                }
                let tooltip = visual.debug_bounds("stacked-tooltip").unwrap();
                assert!(tooltip.left() >= px(12.) && tooltip.right() <= px(width - 12.));
                assert!(tooltip.top() >= px(12.) && tooltip.bottom() <= px(388.));
            }
        }
    }
}

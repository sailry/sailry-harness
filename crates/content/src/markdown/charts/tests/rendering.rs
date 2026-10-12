use super::super::view;
use crate::markdown::style;
use gpui_kit as gpui;
use gpui_kit::component::{ActiveTheme, Root, Theme, ThemeMode};
use gpui_kit::{
    AppContext, Bounds, Context, Hsla, IntoElement, Modifiers, Pixels, Render, SharedString,
    TestAppContext, VisualTestContext, Window, div, point, prelude::*, px, rgb, rgba,
};

struct Fixture(String, f32);

impl Render for Fixture {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(self.1))
            .p(px(8.))
            .debug_selector(|| "chart-stage".into())
            .child(view::render(&self.0, 0, &style::Theme::of(cx), cx).unwrap())
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn painted(cx: &mut VisualTestContext, color: Hsla) -> bool {
    cx.update(|window, _| {
        window
            .painted_quads()
            .iter()
            .any(|quad| quad.background == color.into())
    })
}

fn grid_bounds(cx: &mut VisualTestContext, candlestick: bool) -> Vec<Bounds<Pixels>> {
    cx.update(|window, cx| {
        let color = if candlestick {
            cx.theme().border
        } else {
            cx.theme().chart_grid
        };
        window
            .painted_quads()
            .into_iter()
            .filter(|quad| quad.background == color.into())
            .map(|quad| {
                quad.bounds
                    .map(|pixel| px(pixel.as_f32() / window.scale_factor()))
            })
            .collect()
    })
}

#[gpui::test]
fn cartesian_axes_follow_theme_and_width(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::tests::init(cx);
        cx.set_reduce_motion(true);
    });
    let fixture = cx.new(|_| {
        Fixture(
            r#"{"type":"line","data":[{"label":"A","value":1}]}"#.into(),
            320.,
        )
    });
    let owner = fixture.clone();
    let (_, visual) = cx.add_window_view(|window, cx| Root::new(owner, window, cx));
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| {
            Theme::change(mode, Some(window), cx);
            Theme::update(cx, |theme| {
                theme.chart_grid = rgb(0x8765a3).into();
                theme.border = rgb(0x2c4158).into();
                theme.muted_foreground = rgb(0xc07439).into();
            });
        });
        for width in [320., 760.] {
            for kind in ["line", "bar", "area", "candlestick"] {
                let source = if kind == "candlestick" {
                    r#"{"type":"candlestick","title":"Prices","data":[{"label":"A","open":1002,"high":1005,"low":1001,"close":1004},{"label":"B","open":1003,"high":1006,"low":1000,"close":1001}]}"#.to_owned()
                } else {
                    format!(
                        r#"{{"type":"{kind}","title":"Values","data":[{{"label":"A","value":1000}},{{"label":"B","value":2500}},{{"label":"C","value":1500}}]}}"#
                    )
                };
                visual.update(|_, cx| {
                    fixture.update(cx, |fixture, cx| {
                        fixture.0 = source;
                        fixture.1 = width;
                        cx.notify();
                    });
                });
                draw(visual);
                draw(visual);
                let frame = visual.debug_bounds("markdown-chart-frame-0").unwrap();
                assert_eq!(frame.size.width, px(width - 16.));
                assert_eq!(frame.size.height, px(300.));
                let grid = grid_bounds(visual, kind == "candlestick");
                let horizontal = grid
                    .iter()
                    .filter(|bounds| bounds.size.width > bounds.size.height)
                    .collect::<Vec<_>>();
                let vertical = grid
                    .iter()
                    .filter(|bounds| bounds.size.height > bounds.size.width)
                    .collect::<Vec<_>>();
                assert!(!horizontal.is_empty(), "{kind}: horizontal grid is missing");
                assert!(!vertical.is_empty(), "{kind}: vertical grid is missing");
                assert!(
                    grid.iter().all(|bounds| {
                        bounds.left() >= frame.left() + px(31.)
                            && bounds.right() <= frame.right() + px(1.)
                            && bounds.top() >= frame.top() - px(1.)
                            && bounds.bottom() < frame.bottom() - px(10.)
                    }),
                    "{kind}: the grid must reserve numeric and category label gutters"
                );
                let first = horizontal.iter().map(|bounds| bounds.left()).min().unwrap();
                let last = horizontal
                    .iter()
                    .map(|bounds| bounds.right())
                    .max()
                    .unwrap();
                assert!(
                    (last - first - (frame.size.width - (first - frame.left()))).abs() < px(6.)
                );
                if kind == "candlestick" {
                    let bodies = visual.update(|window, cx| {
                        window
                            .painted_quads()
                            .into_iter()
                            .filter_map(|quad| {
                                let color = quad.background == cx.theme().chart_bullish.into()
                                    || quad.background == cx.theme().chart_bearish.into();
                                let bounds = quad
                                    .bounds
                                    .map(|pixel| px(pixel.as_f32() / window.scale_factor()));
                                (color && bounds.size.width > px(2.) && bounds.size.height > px(2.))
                                    .then_some(bounds)
                            })
                            .collect::<Vec<_>>()
                    });
                    assert_eq!(bodies.len(), 2);
                    assert!(
                        bodies.iter().all(|body| body.size.height > px(70.)),
                        "price scales must retain their domain without a zero baseline"
                    );
                    visual.simulate_mouse_move(bodies[0].center(), None, Modifiers::default());
                    draw(visual);
                    draw(visual);
                    assert!(
                        candle_swatch(visual),
                        "native candle hover must follow the inset price bands"
                    );
                    visual.simulate_mouse_move(
                        point(frame.left() + px(12.), frame.center().y),
                        None,
                        Modifiers::default(),
                    );
                    draw(visual);
                    draw(visual);
                    assert!(
                        !candle_swatch(visual),
                        "numeric label gutters must not activate candle hover"
                    );
                }
                for color in [0x75429c, 0x8765a3] {
                    visual.update(|_, cx| {
                        Theme::update(cx, |theme| {
                            if kind == "candlestick" {
                                theme.border = rgb(color).into();
                            } else {
                                theme.chart_grid = rgb(color).into();
                            }
                        })
                    });
                    draw(visual);
                    assert!(
                        !grid_bounds(visual, kind == "candlestick").is_empty(),
                        "{kind}: grid color did not follow the theme"
                    );
                }
            }
        }
    }
}

fn candle_swatch(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| {
        window.painted_quads().into_iter().any(|quad| {
            let color = quad.background == cx.theme().chart_bullish.into()
                || quad.background == cx.theme().chart_bearish.into();
            let bounds = quad
                .bounds
                .map(|pixel| px(pixel.as_f32() / window.scale_factor()));
            color && bounds.size.width == px(8.) && bounds.size.height == px(8.)
        })
    })
}

struct Bands(gpui_kit::component::plot::shape::BarAlignment, bool);

impl Render for Bands {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::component::chart::BarChart;
        div().w(px(320.)).h(px(300.)).child(
            BarChart::new([("A", 10.), ("B", 20.)])
                .band(|(label, _)| SharedString::from(*label))
                .value(|(_, value)| *value)
                .alignment(self.0)
                .value_axis(true)
                .grid_dashed(false)
                .grid_columns(if self.1 { 4 } else { 0 }),
        )
    }
}

#[gpui::test]
fn perpendicular_grids_support_each_bar_orientation(cx: &mut TestAppContext) {
    use gpui_kit::component::plot::shape::BarAlignment;
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::tests::init(cx);
        Theme::update(cx, |theme| theme.chart_grid = rgb(0x75429c).into());
    });
    let fixture = cx.new(|_| Bands(BarAlignment::Bottom, false));
    let owner = fixture.clone();
    let (_, visual) = cx.add_window_view(|window, cx| Root::new(owner, window, cx));
    for alignment in [
        BarAlignment::Bottom,
        BarAlignment::Top,
        BarAlignment::Left,
        BarAlignment::Right,
    ] {
        for enabled in [false, true] {
            visual.update(|_, cx| {
                fixture.update(cx, |view, cx| {
                    view.0 = alignment;
                    view.1 = enabled;
                    cx.notify();
                })
            });
            draw(visual);
            draw(visual);
            let grid = grid_bounds(visual, false);
            let perpendicular = grid
                .iter()
                .filter(|bounds| {
                    if matches!(alignment, BarAlignment::Bottom | BarAlignment::Top) {
                        bounds.size.height > bounds.size.width
                    } else {
                        bounds.size.width > bounds.size.height
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(perpendicular.len(), if enabled { 4 } else { 0 });
            assert!(
                grid.iter()
                    .all(|bounds| bounds.size.width <= px(320.) && bounds.size.height <= px(300.))
            );
        }
    }
}

#[gpui::test]
fn follows_theme_colors_and_keeps_native_hover(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::tests::init(cx);
        cx.set_reduce_motion(true);
    });
    let fixture = cx.new(|_| {
        Fixture(
            r#"{"type":"line","data":[{"label":"A","value":1}]}"#.into(),
            420.,
        )
    });
    let owner = fixture.clone();
    let (_, visual) = cx.add_window_view(|window, cx| Root::new(owner, window, cx));
    for mode in [ThemeMode::Dark, ThemeMode::Light] {
        visual.update(|window, cx| {
            Theme::change(mode, Some(window), cx);
            Theme::update(cx, |theme| {
                theme.accent = rgba(0xffffff0f).into();
                theme.chart_1 = rgb(0xf59e0b).into();
                theme.chart_2 = rgb(0x3b82f6).into();
                theme.chart_3 = rgb(0x10b981).into();
                theme.chart_4 = rgb(0x8b5cf6).into();
                theme.chart_5 = rgb(0xf43f5e).into();
                theme.chart_bullish = rgb(0x22c55e).into();
                theme.chart_bearish = rgb(0xef4444).into();
            });
        });
        for swapped in [false, true] {
            if swapped {
                visual.update(|_, cx| {
                    Theme::update(cx, |theme| {
                        std::mem::swap(&mut theme.colors.chart_1, &mut theme.colors.chart_2);
                        std::mem::swap(
                            &mut theme.colors.chart_bullish,
                            &mut theme.colors.chart_bearish,
                        );
                    });
                });
            }
            for kind in [
                "line",
                "bar",
                "area",
                "pie",
                "radar",
                "candlestick",
                "sankey",
            ] {
                let source = match kind {
                    "candlestick" => r#"{"type":"candlestick","data":[{"label":"A","open":2,"high":4,"low":1,"close":3},{"label":"B","open":3,"high":4,"low":1,"close":2}]}"#.to_owned(),
                    "sankey" => r#"{"type":"sankey","nodes":[{"id":"a","label":"A"},{"id":"b","label":"B"}],"links":[{"source":"a","target":"b","value":1}]}"#.to_owned(),
                    _ => format!(r#"{{"type":"{kind}","data":[{{"label":"A","value":1}},{{"label":"B","value":1}},{{"label":"C","value":1}}]}}"#),
                };
                visual.simulate_mouse_move(point(px(430.), px(330.)), None, Modifiers::default());
                visual.update(|_, cx| {
                    fixture.update(cx, |fixture, cx| {
                        fixture.0 = source;
                        cx.notify();
                    });
                });
                draw(visual);
                draw(visual);
                let bounds = visual.debug_bounds("chart-stage").unwrap();
                assert!(bounds.size.width > px(300.) && bounds.size.height >= px(300.));
                let expected = visual.update(|_, cx| match kind {
                    "candlestick" => vec![cx.theme().chart_bullish, cx.theme().chart_bearish],
                    "sankey" | "pie" => vec![cx.theme().chart_1, cx.theme().chart_2],
                    "radar" => vec![cx.theme().chart_1],
                    _ => vec![cx.theme().chart_2],
                });
                if matches!(kind, "bar" | "candlestick" | "sankey") {
                    for color in &expected {
                        assert!(
                            painted(visual, *color),
                            "{kind}: chart fill must use its semantic color"
                        );
                    }
                } else {
                    assert!(expected.iter().all(|color| !painted(visual, *color)));
                    let mut observed = vec![false; expected.len()];
                    for (x, y) in [(0., 0.), (70., 0.), (-70., 0.), (0., 70.), (0., -70.)] {
                        visual.simulate_mouse_move(
                            bounds.center() + point(px(x), px(y)),
                            None,
                            Modifiers::default(),
                        );
                        draw(visual);
                        draw(visual);
                        for (seen, color) in observed.iter_mut().zip(&expected) {
                            *seen |= painted(visual, *color);
                        }
                    }
                    assert!(
                        observed.iter().all(|seen| *seen),
                        "{kind}: native hover must show the chart palette"
                    );
                    visual.simulate_mouse_move(
                        point(px(430.), px(330.)),
                        None,
                        Modifiers::default(),
                    );
                    draw(visual);
                    draw(visual);
                    assert!(
                        expected.iter().all(|color| !painted(visual, *color)),
                        "{kind}: native hover must clear outside the plot"
                    );
                }
            }
        }
    }
}

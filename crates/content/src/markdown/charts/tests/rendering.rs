use super::super::view;
use crate::markdown::style;
use gpui_kit as gpui;
use gpui_kit::component::{ActiveTheme, Root, Theme, ThemeMode};
use gpui_kit::{
    AppContext, Context, Hsla, IntoElement, Modifiers, Render, TestAppContext, VisualTestContext,
    Window, div, point, prelude::*, px, rgb, rgba,
};

struct Fixture(String);

impl Render for Fixture {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(420.))
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

#[gpui::test]
fn follows_theme_colors_and_keeps_native_hover(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::tests::init(cx);
        cx.set_reduce_motion(true);
    });
    let fixture = cx.new(|_| Fixture(r#"{"type":"line","data":[{"label":"A","value":1}]}"#.into()));
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

use core::prelude::v1::test;
use gpui_kit::component::{
    tab::{Tab, TabBar, TabVariant},
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::time::Duration;

struct Tabs {
    selected: usize,
    variant: TabVariant,
    equal: bool,
    width: f32,
}

impl Render for Tabs {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex().w(px(self.width)).child(
            div()
                .debug_selector(|| "tabs-box".into())
                .when(self.equal, |wrapper| wrapper.w_full())
                .child(
                    TabBar::new("test-tabs")
                        .with_variant(self.variant)
                        .when(self.equal, |bar| bar.equal_width().w_full())
                        .selected_index(self.selected)
                        .children(["One", "Two", "Three"].into_iter().enumerate().map(
                            |(index, label)| {
                                Tab::new()
                                    .label(label)
                                    .debug_selector(move || format!("tab-{index}"))
                            },
                        ))
                        .on_click(cx.listener(|view, index, _, cx| {
                            view.selected = *index;
                            cx.notify();
                        })),
                ),
        )
    }
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..4 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.executor().advance_clock(Duration::from_millis(250));
    }
}

fn setup(cx: &mut TestAppContext) -> (Entity<Tabs>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
    });
    let mut view = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let tabs = cx.new(|_| Tabs {
            selected: 0,
            variant: TabVariant::Segmented,
            equal: false,
            width: 240.,
        });
        view = Some(tabs.clone());
        Root::new(tabs, window, cx)
    });
    (view.unwrap(), visual)
}

#[gpui::test]
fn insets_and_indicator(cx: &mut TestAppContext) {
    let (view, visual) = setup(cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for equal in [false, true] {
            for width in [240., 640.] {
                visual.update(|window, cx| {
                    Theme::change(mode, Some(window), cx);
                    view.update(cx, |view, cx| {
                        view.equal = equal;
                        view.width = width;
                        cx.notify();
                    });
                });
                settle(visual);
                for index in [0, 2, 1, 0] {
                    let selector = ["tab-0", "tab-1", "tab-2"][index];
                    let tab = visual.debug_bounds(selector).unwrap();
                    visual.simulate_click(tab.center(), Modifiers::none());
                    settle(visual);
                    assert_eq!(view.read_with(visual, |view, _| view.selected), index);
                    let bar = visual.debug_bounds("tabs-box").unwrap();
                    let first = visual.debug_bounds("tab-0").unwrap();
                    let last = visual.debug_bounds("tab-2").unwrap();
                    let left = first.left() - bar.left();
                    let right = bar.right() - last.right();
                    assert!(
                        left >= px(3.) && right >= px(3.),
                        "equal={equal}, width={width}, insets={left:?}/{right:?}"
                    );
                    assert!((left - right).abs() <= px(1.));
                    let selected = visual.debug_bounds(selector).unwrap();
                    let indicator = visual.debug_bounds("tab-indicator").unwrap();
                    assert!((selected.left() - indicator.left()).abs() <= px(1.));
                    assert!((selected.right() - indicator.right()).abs() <= px(1.));
                    if equal {
                        assert!((first.size.width - last.size.width).abs() <= px(1.));
                    }
                }
            }
        }
    }
}

#[gpui::test]
fn equal_width_variants(cx: &mut TestAppContext) {
    let (view, visual) = setup(cx);
    for variant in [
        TabVariant::Tab,
        TabVariant::Outline,
        TabVariant::Pill,
        TabVariant::Segmented,
        TabVariant::Underline,
    ] {
        for width in [240., 640.] {
            visual.update(|_, cx| {
                view.update(cx, |view, cx| {
                    view.variant = variant;
                    view.equal = true;
                    view.width = width;
                    cx.notify();
                });
            });
            settle(visual);
            let bar = visual.debug_bounds("tabs-box").unwrap();
            let tabs =
                ["tab-0", "tab-1", "tab-2"].map(|selector| visual.debug_bounds(selector).unwrap());
            for tab in tabs {
                assert!(
                    tab.left() >= bar.left() - px(1.) && tab.right() <= bar.right() + px(1.),
                    "variant={variant:?}, width={width}, tab={tab:?}, bar={bar:?}"
                );
                assert!((tab.size.width - tabs[0].size.width).abs() <= px(1.));
                assert!(tab.size.width > bar.size.width / 5.);
            }
            for index in [2, 0] {
                visual.simulate_click(tabs[index].center(), Modifiers::none());
                settle(visual);
                assert_eq!(view.read_with(visual, |view, _| view.selected), index);
            }
        }
    }
}

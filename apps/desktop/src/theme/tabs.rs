use core::prelude::v1::test;
use gpui_kit::component::{
    tab::{Tab, TabBar, TabVariant},
    *,
};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::*;
use std::time::Duration;

struct Tabs {
    selected: usize,
}

impl Render for Tabs {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex().w(px(240.)).child(
            div()
                .id("tabs-box")
                .test_support()
                .debug_selector(|| "tabs-box".into())
                .child(
                    TabBar::new("test-tabs")
                        .with_variant(TabVariant::Segmented)
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
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        });
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
        let tabs = cx.new(|_| Tabs { selected: 0 });
        view = Some(tabs.clone());
        Root::new(tabs, window, cx)
    });
    (view.unwrap(), visual)
}

#[gpui::test]
fn themed_categories(cx: &mut TestAppContext) {
    let (view, visual) = setup(cx);
    let directory = tempfile::tempdir().unwrap();
    let source = super::fixture::styled(directory.path(), "tabs");
    let loaded = super::package::Loaded::read(&source, true).unwrap();
    visual.update(|_, cx| {
        cx.global_mut::<super::Catalog>()
            .packages
            .push(loaded.into())
    });
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| {
            Theme::change(mode, Some(window), cx);
            super::apply(1, Some(window), cx);
            let colors = &super::variant(cx).unwrap().theme.colors;
            assert_eq!(
                cx.theme().tab_foreground,
                theme::try_parse_color(colors.tab_foreground.as_ref().unwrap()).unwrap()
            );
            assert_eq!(
                cx.theme().tab_active_foreground,
                theme::try_parse_color(colors.tab_active_foreground.as_ref().unwrap()).unwrap()
            );
            assert_ne!(cx.theme().tab_foreground, cx.theme().tab_active_foreground);
            assert!(cx.theme().tab_bar_segmented.a < 1.);
            assert_ne!(cx.theme().tokens.button, cx.theme().tokens.button_primary);
            view.update(cx, |_, cx| cx.notify());
        });
        settle(visual);
        for index in [2, 0] {
            visual.update(|window, cx| {
                window
                    .within("tabs-box")
                    .within("gpui_base::tabs::Tab")
                    .click(index, cx);
            });
            settle(visual);
            assert_eq!(view.read_with(visual, |view, _| view.selected), index);
        }
    }
}

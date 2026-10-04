use super::*;
use core::prelude::v1::test;

struct View {
    loading: bool,
    card: bool,
    populated: bool,
    heading: bool,
    clicks: usize,
}

impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().p_4().child(
            Group::new("settings_memory")
                .card(self.card)
                .heading(self.heading)
                .loading(self.loading)
                .when(self.populated, |group| {
                    group.child(
                        div().py_3().child(
                            Button::new("row-action")
                                .label("Action")
                                .debug_selector(|| "row-action".into())
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.clicks += 1;
                                    cx.notify();
                                })),
                        ),
                    )
                }),
        )
    }
}

#[gpui::test]
fn mask_covers_surface_without_changing_content(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut view = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let entity = cx.new(|_| View {
            loading: false,
            card: true,
            populated: false,
            heading: true,
            clicks: 0,
        });
        view = Some(entity.clone());
        Root::new(entity, window, cx)
    });
    let view = view.unwrap();
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [320., 960.] {
            let handle = visual.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            visual.simulate_window_resize(handle, size(px(width), px(640.)));
            for card in [false, true] {
                for populated in [false, true] {
                    for heading in [false, true] {
                        view.update(visual, |view, cx| {
                            view.card = card;
                            view.populated = populated;
                            view.heading = heading;
                            view.loading = false;
                            cx.notify();
                        });
                        draw(visual);
                        let surface = visual
                            .debug_bounds("settings-surface-settings_memory")
                            .unwrap();
                        let rows = visual
                            .debug_bounds("settings-group-settings_memory")
                            .unwrap();
                        let inset = if card || !populated { px(16.) } else { px(0.) };
                        assert_eq!(rows.left() - surface.left(), inset);
                        assert_eq!(surface.right() - rows.right(), inset);
                        view.update(visual, |view, cx| {
                            view.loading = true;
                            cx.notify();
                        });
                        draw(visual);
                        assert_eq!(
                            visual.debug_bounds("settings-surface-settings_memory"),
                            Some(surface)
                        );
                        assert_eq!(
                            visual.debug_bounds("settings-group-settings_memory"),
                            Some(rows)
                        );
                        assert_eq!(
                            visual.debug_bounds("settings-loading-settings_memory"),
                            Some(surface)
                        );
                        if populated {
                            let action = visual.debug_bounds("row-action").unwrap();
                            let before = view.read_with(visual, |view, _| view.clicks);
                            visual.simulate_click(action.center(), Modifiers::default());
                            assert_eq!(view.read_with(visual, |view, _| view.clicks), before);
                            view.update(visual, |view, cx| {
                                view.loading = false;
                                cx.notify();
                            });
                            draw(visual);
                            assert!(
                                visual
                                    .debug_bounds("settings-loading-settings_memory")
                                    .is_none()
                            );
                            visual.simulate_click(action.center(), Modifiers::default());
                            assert_eq!(view.read_with(visual, |view, _| view.clicks), before + 1);
                        }
                    }
                }
            }
        }
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

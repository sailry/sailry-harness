//! Existing panel, list and card placements share Kit's native empty-state slots.
use gpui_kit::component::empty::{Empty, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle};
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{component::*, *};

pub(crate) fn panel(icon: IconName, label: &'static str, cx: &App) -> Div {
    content(icon.into(), crate::tr(label), label.into(), cx)
}

pub(crate) fn content(icon: Icon, label: SharedString, id: SharedString, cx: &App) -> Div {
    v_flex()
        .size_full()
        .min_h_0()
        .items_center()
        .justify_center()
        .p_6()
        .child(message(icon, label, id, cx).max_w_80())
}

pub(crate) fn list(icon: IconName, label: &'static str, cx: &App) -> Div {
    list_content(icon.into(), crate::tr(label), label.into(), cx)
}

pub(crate) fn list_content(icon: Icon, label: SharedString, id: SharedString, cx: &App) -> Div {
    message(icon, label, id, cx).flex_shrink_0().py_10()
}

fn message(icon: Icon, label: SharedString, id: SharedString, cx: &App) -> Div {
    let title_id = id.clone();
    let icon_id = id.clone();
    v_flex()
        .w_full()
        .min_w_0()
        .items_center()
        .justify_center()
        .gap_3()
        .text_center()
        .debug_selector(move || format!("empty-{id}"))
        .child(
            Empty::new().flex_none().p_0().header(
                EmptyHeader::new()
                    .max_w_full()
                    .gap_3()
                    .media(
                        EmptyMedia::new()
                            .with_variant(EmptyMediaVariant::Icon)
                            .size_12()
                            .rounded_full()
                            .mb_0()
                            .child(
                                h_flex()
                                    .debug_selector(move || format!("empty-icon-{icon_id}"))
                                    .size_full()
                                    .items_center()
                                    .justify_center()
                                    .child(icon.size_6()),
                            ),
                    )
                    .title(
                        EmptyTitle::new()
                            .font_weight(FontWeight::NORMAL)
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                div()
                                    .debug_selector(move || format!("empty-title-{title_id}"))
                                    .child(label),
                            ),
                    ),
            ),
        )
}

pub(crate) fn card(icon: IconName, label: &'static str, cx: &App) -> Div {
    card_content(list(icon, label, cx), label.into(), cx)
}

pub(crate) fn card_content(body: impl IntoElement, id: SharedString, cx: &App) -> Div {
    card_height(body, id, false, cx)
}

pub(crate) fn card_height(
    body: impl IntoElement,
    id: SharedString,
    fill_height: bool,
    cx: &App,
) -> Div {
    card_aligned(body, id, fill_height, false, cx)
}

pub(crate) fn card_aligned(
    body: impl IntoElement,
    id: SharedString,
    fill_height: bool,
    start: bool,
    cx: &App,
) -> Div {
    let mut style = StyleRefinement::default()
        .p_0()
        .gap_0()
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius_lg);
    if fill_height {
        style = style.h_full().min_h_0();
    }
    div()
        .debug_selector(move || format!("empty-card-{id}"))
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .when(fill_height, |card| card.h_full().min_h_0())
        .child(
            GroupBox::new()
                .fill()
                .when(fill_height, |card| card.h_full().min_h_0())
                .content_style(style)
                .child(if fill_height {
                    v_flex()
                        .h_full()
                        .min_h_0()
                        .justify_center()
                        .when(start, |body| body.justify_start())
                        .child(body)
                        .into_any_element()
                } else {
                    body.into_any_element()
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::component::button::{Button, ButtonVariants};

    struct View;

    struct Lists;

    impl Render for Lists {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            v_flex()
                .size_full()
                .p_6()
                .gap_4()
                .child(card(IconName::Folder, "resource_project_empty", cx))
                .child(list(IconName::Bell, "notifications_empty", cx))
        }
    }

    #[gpui::test]
    fn list_cards_stay_content_sized(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
        });
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| Lists);
            Root::new(view, window, cx)
        });
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            for width in [320., 960.] {
                let handle = visual.update(|window, cx| {
                    Theme::change(mode, Some(window), cx);
                    window.window_handle()
                });
                visual.simulate_window_resize(handle, size(px(width), px(640.)));
                visual.run_until_parked();
                visual.update(|window, cx| {
                    let _ = window.draw(cx);
                });
                let card = visual
                    .debug_bounds("empty-card-resource_project_empty")
                    .unwrap();
                let body = visual.debug_bounds("empty-resource_project_empty").unwrap();
                let list = visual.debug_bounds("empty-notifications_empty").unwrap();
                assert_eq!(card.size.width, px(width - 48.));
                assert_eq!(card.size.height, body.size.height + px(2.));
                assert_eq!(list.size.height, body.size.height);
                assert!(card.size.height < px(180.));
                assert_eq!(list.top() - card.bottom(), px(16.));
                for (icon, title) in [
                    (
                        "empty-icon-resource_project_empty",
                        "empty-title-resource_project_empty",
                    ),
                    (
                        "empty-icon-notifications_empty",
                        "empty-title-notifications_empty",
                    ),
                ] {
                    let icon = visual.debug_bounds(icon).unwrap();
                    let title = visual.debug_bounds(title).unwrap();
                    assert_eq!(icon.size, size(px(48.), px(48.)));
                    assert!(title.size.height <= px(24.));
                    assert_eq!(title.top() - icon.bottom(), px(12.));
                    assert!((icon.center().x - title.center().x).abs() < px(1.));
                }
            }
        }
    }

    impl Render for View {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            panel(IconName::Folder, "resource_project_empty", cx)
                .gap_4()
                .child(
                    Button::new("create")
                        .debug_selector(|| "empty-action".into())
                        .primary()
                        .label(crate::tr("project_new")),
                )
        }
    }

    #[gpui::test]
    fn title_and_action_stay_compact(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
        });
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| View);
            Root::new(view, window, cx)
        });
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            for width in [320., 960.] {
                let handle = visual.update(|window, cx| {
                    Theme::change(mode, Some(window), cx);
                    window.window_handle()
                });
                visual.simulate_window_resize(handle, size(px(width), px(640.)));
                visual.run_until_parked();
                visual.update(|window, cx| {
                    let _ = window.draw(cx);
                });
                let content = visual.debug_bounds("empty-resource_project_empty").unwrap();
                let title = visual
                    .debug_bounds("empty-title-resource_project_empty")
                    .unwrap();
                let action = visual.debug_bounds("empty-action").unwrap();
                assert!((content.bottom() - title.bottom()).abs() < px(1.));
                assert!(title.size.height <= px(24.));
                let icon = visual
                    .debug_bounds("empty-icon-resource_project_empty")
                    .unwrap();
                assert_eq!(icon.size, size(px(48.), px(48.)));
                assert!((icon.center().x - title.center().x).abs() < px(1.));
                assert_eq!(title.top() - icon.bottom(), px(12.));
                assert!(action.top() > title.bottom());
                assert!(action.top() - title.bottom() <= px(16.));
                assert!((action.center().x - content.center().x).abs() < px(1.));
                assert!(content.left() >= px(0.) && content.right() <= px(width));
            }
        }
    }
}

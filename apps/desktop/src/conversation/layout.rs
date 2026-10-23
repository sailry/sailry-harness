use gpui_kit::component::*;
use gpui_kit::*;

pub(crate) const USER_MESSAGE_WIDTH: f32 = 440.;

pub(super) fn composer_group(view: EntityId) -> SharedString {
    format!("composer-controls-{view:?}").into()
}

pub(super) fn composer(
    notices: impl IntoIterator<Item = AnyElement>,
    content: AnyElement,
    tray: Option<AnyElement>,
    context: Option<AnyElement>,
    menu: Option<AnyElement>,
    sidebar: bool,
    view: EntityId,
    cx: &App,
) -> AnyElement {
    let group = composer_group(view);
    v_flex()
        .debug_selector(|| "conversation-composer".into())
        .flex_shrink_1()
        .min_h_0()
        .w_full()
        .max_w(px(super::CONTENT_WIDTH))
        .mx_auto()
        .px(if sidebar { px(12.) } else { px(24.) })
        .pb_4()
        .pt_2()
        .gap_2()
        .children(notices)
        .child(
            v_flex()
                .id(group.clone())
                .group(group)
                .flex_shrink_0()
                .relative()
                .w_full()
                .children(menu)
                .children(tray)
                .child(
                    input_surface(cx)
                        .debug_selector(|| "composer-surface".into())
                        .child(content),
                )
                .children(context),
        )
        .into_any_element()
}

pub(crate) fn input_surface(cx: &App) -> Div {
    crate::theme::composer_surface(cx)
}

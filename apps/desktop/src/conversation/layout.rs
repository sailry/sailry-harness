use gpui_kit::component::*;
use gpui_kit::*;

pub(crate) const USER_MESSAGE_WIDTH: f32 = 440.;

pub(super) fn composer(
    notices: impl IntoIterator<Item = AnyElement>,
    content: AnyElement,
    context: Option<AnyElement>,
    menu: Option<AnyElement>,
    sidebar: bool,
    cx: &App,
) -> AnyElement {
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
                .flex_shrink_0()
                .relative()
                .w_full()
                .children(menu)
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
    v_flex()
        .relative()
        .w_full()
        .p_3()
        .rounded(cx.theme().radius_2xl())
        .bg(cx.theme().muted)
}

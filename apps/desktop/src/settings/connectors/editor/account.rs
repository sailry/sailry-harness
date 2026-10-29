//! The local CLI account a connector binds, presented as a one-row selection list.
use super::*;
use gpui_kit::component::{avatar::Avatar, radio::Radio};

const ROW_HEIGHT: f32 = 52.;
const AVATAR_SIZE: f32 = 28.;

pub(super) fn list(identity: Option<&connector::Identity>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let row = h_flex()
        .w_full()
        .h(px(ROW_HEIGHT))
        .px_3p5()
        .gap_3()
        .items_center();
    let row = match identity {
        Some(identity) => row
            .bg(theme.accent)
            .child(
                Avatar::new()
                    .name(identity.name.clone())
                    .with_size(px(AVATAR_SIZE)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.foreground)
                            .truncate()
                            .child(identity.name.clone()),
                    )
                    .when(identity.id != identity.name, |text| {
                        text.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .truncate()
                                .child(identity.id.clone()),
                        )
                    }),
            )
            .child(
                Radio::new("connector-account")
                    .checked(true)
                    .accessibility_label(identity.name.clone()),
            ),
        None => row
            .child(
                div()
                    .size(px(AVATAR_SIZE))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(IconName::CircleAlert).text_color(theme.muted_foreground)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(tr("connector_not_connected")),
            ),
    };
    div()
        .w_full()
        .bg(theme.colors.list)
        .border_1()
        .border_color(theme.border)
        .rounded(theme.radius_2xl())
        .overflow_hidden()
        .child(row)
        .into_any_element()
}

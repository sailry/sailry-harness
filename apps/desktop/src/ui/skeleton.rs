//! Loading blocks share Kit's semantic skeleton color and radius.
use gpui_kit::{
    App, Styled as _,
    component::{ActiveTheme as _, skeleton::Skeleton},
};

pub(crate) fn block(cx: &App) -> Skeleton {
    Skeleton::new()
        .bg(cx.theme().skeleton)
        .rounded(cx.theme().radius)
}

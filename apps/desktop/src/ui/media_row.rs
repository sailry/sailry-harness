//! Leading tile with a title and muted one-line description, shared by settings lists
//! so every record uses the same sizes and spacing.
use gpui_kit::{
    component::{ActiveTheme as _, h_flex, v_flex},
    prelude::FluentBuilder as _,
    *,
};

/// Edge of the leading tile slot; callers size their tile or icon surface to it.
pub(crate) const TILE: f32 = 36.;

#[derive(IntoElement)]
pub(crate) struct MediaRow {
    title: SharedString,
    description: Option<SharedString>,
    leading: Option<AnyElement>,
    badge: Option<AnyElement>,
}

impl MediaRow {
    pub(crate) fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: None,
            leading: None,
            badge: None,
        }
    }

    pub(crate) fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    pub(crate) fn leading(mut self, element: impl IntoElement) -> Self {
        self.leading = Some(element.into_any_element());
        self
    }

    /// A small element after the title, such as a Kit Tag.
    pub(crate) fn badge(mut self, element: impl IntoElement) -> Self {
        self.badge = Some(element.into_any_element());
        self
    }
}

impl RenderOnce for MediaRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap_3()
            .when_some(self.leading, |row, leading| {
                row.child(
                    h_flex()
                        .size(px(TILE))
                        .flex_shrink_0()
                        .items_center()
                        .justify_center()
                        .child(leading),
                )
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .line_height(relative(1.3))
                    .child(
                        h_flex()
                            .min_w_0()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(theme.font_size)
                                    .font_medium()
                                    .text_color(theme.foreground)
                                    .child(self.title),
                            )
                            .when_some(self.badge, |title, badge| {
                                title.child(div().flex_shrink_0().child(badge))
                            }),
                    )
                    .when_some(self.description, |column, description| {
                        column.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(description),
                        )
                    }),
            )
    }
}

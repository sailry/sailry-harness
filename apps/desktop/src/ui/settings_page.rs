//! Kit b79f4ce SettingPage accepts typed groups, not arbitrary product content.
//! Share Sailry's accepted page composition while Kit owns scrolling and styles.
use gpui_kit::{
    component::{scroll::ScrollableElement, *},
    prelude::FluentBuilder as _,
    *,
};

#[derive(IntoElement)]
pub(crate) struct SettingsPage {
    id: SharedString,
    title: SharedString,
    description: Option<SharedString>,
    leading: Vec<AnyElement>,
    children: Vec<AnyElement>,
    key: usize,
}

impl SettingsPage {
    pub(crate) fn new(id: impl Into<SharedString>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: None,
            leading: Vec::new(),
            children: Vec::new(),
            key: 0,
        }
    }

    pub(crate) fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub(crate) fn key(mut self, key: usize) -> Self {
        self.key = key;
        self
    }

    pub(crate) fn leading(mut self, leading: impl IntoIterator<Item = AnyElement>) -> Self {
        self.leading.extend(leading);
        self
    }
}

impl ParentElement for SettingsPage {
    fn extend(&mut self, children: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(children);
    }
}

impl RenderOnce for SettingsPage {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let page = format!("{}-viewport", self.id);
        let content = format!("{}-content", self.id);
        let header = format!("{}-header", self.id);
        let heading = format!("{}-heading", self.id);
        let description = format!("{}-description", self.id);
        v_flex()
            .id((self.id, self.key))
            .debug_selector(move || page.clone())
            .size_full()
            .min_h_0()
            .min_w_0()
            .text_color(cx.theme().foreground)
            .overflow_y_scrollbar()
            .child(
                v_flex()
                    .debug_selector(move || content.clone())
                    .w_full()
                    .max_w(px(crate::preview::CONTENT_WIDTH))
                    .mx_auto()
                    .p_6()
                    .gap_6()
                    .children(self.leading)
                    .child(
                        v_flex()
                            .debug_selector(move || header.clone())
                            .gap_2()
                            .child(
                                div()
                                    .debug_selector(move || heading.clone())
                                    .text_lg()
                                    .font_semibold()
                                    .child(self.title),
                            )
                            .when_some(self.description, |header, text| {
                                header.child(
                                    div()
                                        .debug_selector(move || description.clone())
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(text),
                                )
                            }),
                    )
                    .children(self.children),
            )
    }
}

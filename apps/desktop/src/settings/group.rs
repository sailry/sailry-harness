use gpui_kit::component::{
    button::Button,
    group_box::{GroupBox, GroupBoxVariants},
    separator::Separator,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tr;

/// Settings-specific composition; Kit still owns the card surface and controls.
#[derive(IntoElement)]
pub struct Group {
    key: &'static str,
    rows: Vec<AnyElement>,
    actions: Vec<Button>,
    card: bool,
    heading: bool,
    loading: bool,
    empty: (IconName, &'static str),
}

impl Group {
    pub fn new(key: &'static str) -> Self {
        Self {
            key,
            rows: Vec::new(),
            actions: Vec::new(),
            card: true,
            heading: true,
            loading: false,
            empty: (IconName::Inbox, "settings_empty"),
        }
    }

    pub fn empty(mut self, icon: IconName, label: &'static str) -> Self {
        self.empty = (icon, label);
        self
    }

    #[cfg(test)]
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn heading(mut self, visible: bool) -> Self {
        self.heading = visible;
        self
    }

    pub fn card(mut self, card: bool) -> Self {
        self.card = card;
        self
    }

    pub fn child(mut self, row: impl IntoElement) -> Self {
        self.rows.push(row.into_any_element());
        self
    }

    pub fn action(mut self, button: Button) -> Self {
        self.actions.push(button);
        self
    }

    pub fn children(mut self, rows: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.rows
            .extend(rows.into_iter().map(IntoElement::into_any_element));
        self
    }
}

impl RenderOnce for Group {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let key = self.key;
        let card = self.card || self.rows.is_empty();
        let mut content_style = StyleRefinement::default().p_0().gap_0();
        if card {
            content_style = content_style
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius_lg);
        }
        let mut rows = v_flex()
            .relative()
            .w_full()
            .min_w_0()
            .gap_0()
            .debug_selector(move || format!("settings-group-{key}"));
        if self.rows.is_empty() {
            rows = rows.child(crate::empty_state::list(self.empty.0, self.empty.1, cx));
        }
        for (index, row) in self.rows.into_iter().enumerate() {
            if index > 0 {
                // Kit paints an absolute rule; each divider needs its own layout box.
                rows = rows.child(
                    div()
                        .relative()
                        .w_full()
                        .h(px(1.))
                        .flex_shrink_0()
                        .debug_selector(move || format!("settings-divider-{key}-{index}"))
                        .child(Separator::horizontal().size_full()),
                );
            }
            rows = rows.child(row);
        }
        // Keep the inset with the rows, not with the surface the mask covers.
        let mut surface = div()
            .relative()
            .w_full()
            .min_w_0()
            .debug_selector(move || format!("settings-surface-{key}"))
            .when(card, |surface| {
                surface.rounded(cx.theme().radius_lg).overflow_hidden()
            })
            .child(div().when(card, |content| content.px_4()).child(rows));
        if self.loading {
            surface = surface.child(
                div()
                    .id(SharedString::from(format!("settings-loading-{key}")))
                    .debug_selector(move || format!("settings-loading-{key}"))
                    .absolute()
                    .inset_0()
                    .occlude()
                    .bg(cx.theme().group_box.opacity(0.8))
                    .rounded(cx.theme().radius_lg)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(spinner::Spinner::new()),
            );
        }
        GroupBox::new()
            .id(key)
            .when(card, |group| group.fill())
            .gap_2()
            .when(self.heading, |group| {
                group.title(
                    h_flex()
                        .debug_selector(move || format!("settings-heading-{key}"))
                        .w_full()
                        .min_h_7()
                        .gap_3()
                        .flex_wrap()
                        .child(
                            div()
                                .debug_selector(move || format!("settings-title-{key}"))
                                .child(tr(key)),
                        )
                        .when(!self.actions.is_empty(), |header| {
                            header.child(
                                h_flex()
                                    .debug_selector(move || format!("settings-actions-{key}"))
                                    .ml_auto()
                                    .gap_2()
                                    .flex_wrap()
                                    .justify_end()
                                    .children(self.actions),
                            )
                        }),
                )
            })
            .title_style(
                StyleRefinement::default()
                    .text_sm()
                    .text_color(cx.theme().foreground),
            )
            .content_style(content_style)
            .child(surface)
    }
}

#[cfg(test)]
mod tests;

#[derive(IntoElement)]
pub struct Row {
    key: &'static str,
    description: Option<SharedString>,
    control: AnyElement,
    wide: bool,
}

impl Row {
    pub fn new(key: &'static str, control: impl IntoElement) -> Self {
        Self {
            key,
            description: None,
            control: control.into_any_element(),
            wide: false,
        }
    }

    pub fn description(mut self, key: &str) -> Self {
        self.description = Some(tr(key));
        self
    }

    pub fn wide(mut self) -> Self {
        self.wide = true;
        self
    }
}

impl RenderOnce for Row {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let key = self.key;
        // Field places descriptions beneath its input column. Settings needs them
        // beneath the label, with a trailing control and wrapping wide inputs.
        let label = v_flex()
            .flex_1()
            .min_w_0()
            .gap_1()
            .when(self.wide, |label| label.min_w(px(160.)))
            .child(
                h_flex()
                    .debug_selector(move || format!("settings-label-{key}"))
                    .gap_2()
                    .text_color(cx.theme().group_box_foreground)
                    .child(tr(key)),
            )
            .when_some(self.description, |label, description| {
                label.child(
                    div()
                        .debug_selector(move || format!("settings-description-{key}"))
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                )
            });
        h_flex()
            .debug_selector(move || format!("settings-row-{key}"))
            .w_full()
            .py_3()
            .gap_4()
            .text_sm()
            .when(self.wide, |row| row.flex_wrap())
            .child(label)
            .child(
                h_flex()
                    .debug_selector(move || format!("settings-control-{key}"))
                    .flex_shrink_0()
                    .min_w_0()
                    .max_w_full()
                    .justify_end()
                    .when(self.wide, |control| control.w_64().ml_auto())
                    .child(self.control),
            )
    }
}

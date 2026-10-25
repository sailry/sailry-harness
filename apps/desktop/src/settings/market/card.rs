use super::*;
use crate::ui::skeleton::block;
use gpui_kit::assets::IconName;

pub(super) fn content(
    title: SharedString,
    description: String,
    icon: Option<AnyElement>,
    source: Option<SharedString>,
    cx: &App,
) -> AnyElement {
    h_flex()
        .w_full()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap_3()
        .children(icon)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .items_start()
                .gap_1()
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .text_left()
                        .truncate()
                        .font_medium()
                        .child(title),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .text_left()
                        .truncate()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(match source {
                            Some(source) if !description.is_empty() => {
                                format!("{source} · {description}")
                            }
                            Some(source) => source.to_string(),
                            None => description,
                        }),
                ),
        )
        .into_any_element()
}

pub(super) fn row(
    id: String,
    summary: impl IntoElement,
    action: impl IntoElement,
    cx: &App,
) -> list::ListItem {
    list::ListItem::new(SharedString::from(id))
        .w_full()
        .h(px(76.))
        .text_sm()
        .min_w_0()
        .px_4()
        .gap_3()
        .items_center()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().group_box)
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .items_center()
                .gap_3()
                .child(summary)
                .child(action),
        )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Install,
    Installed,
    Update,
}

impl Action {
    pub(super) fn new(installed: bool, update: bool) -> Self {
        match (installed, update) {
            (false, _) => Self::Install,
            (true, true) => Self::Update,
            (true, false) => Self::Installed,
        }
    }

    pub(super) fn label(self) -> &'static str {
        if self == Self::Update {
            "market_update"
        } else {
            "market_install"
        }
    }
}

pub(super) fn action(id: String, action: Action, _cx: &App) -> Button {
    Button::new(SharedString::from(id))
        .secondary()
        .small()
        .size_8()
        .flex_shrink_0()
        .rounded_md()
        .icon(if action == Action::Update {
            IconName::Redo
        } else {
            IconName::Download
        })
        .when(action == Action::Update, |button| {
            button.primary().outline()
        })
        .disabled(action == Action::Installed)
        .tooltip(tr(action.label()))
        .accessibility_label(tr(action.label()))
}

pub(super) fn skeleton(cx: &App) -> AnyElement {
    div()
        .grid()
        .grid_cols(2)
        .gap_3()
        .w_full()
        .children((0usize..6).map(|index| {
            h_flex()
                .id(("market-skeleton", index))
                .h(px(76.))
                .px_4()
                .gap_3()
                .items_center()
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius_lg)
                .child(block(cx).size_10())
                .child(
                    v_flex()
                        .flex_1()
                        .gap_2()
                        .child(block(cx).w_1_2().h_4())
                        .child(block(cx).w_full().h_3()),
                )
        }))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn installation_controls_available_actions() {
        assert_eq!(Action::new(false, false), Action::Install);
        assert_eq!(Action::new(false, true), Action::Install);
        assert_eq!(Action::new(true, false), Action::Installed);
        assert_eq!(Action::new(true, true), Action::Update);
    }
}

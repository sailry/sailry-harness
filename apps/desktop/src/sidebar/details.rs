use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{component::*, *};

pub(super) struct Details {
    pub title: SharedString,
    pub project: Option<SharedString>,
    pub branch: Option<SharedString>,
    pub path: Option<SharedString>,
    pub host: SharedString,
}

impl Details {
    pub(super) fn render(&self, window: &Window, cx: &App) -> Div {
        v_flex()
            .w(px(320.).min(window.viewport_size().width - px(48.)))
            .min_w_0()
            .gap_2()
            .text_sm()
            .child(
                div()
                    .font_medium()
                    .whitespace_normal()
                    .child(self.title.clone()),
            )
            .when(self.project.is_some() || self.branch.is_some(), |card| {
                card.child(
                    h_flex()
                        .items_start()
                        .gap_2()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            Icon::new(gpui_kit::assets::IconName::GitBranch)
                                .size_4()
                                .flex_shrink_0(),
                        )
                        .child(
                            v_flex()
                                .min_w_0()
                                .children(
                                    self.project
                                        .clone()
                                        .map(|project| div().whitespace_normal().child(project)),
                                )
                                .children(
                                    self.branch
                                        .clone()
                                        .map(|branch| div().whitespace_normal().child(branch)),
                                ),
                        ),
                )
            })
            .child(
                h_flex()
                    .items_start()
                    .gap_2()
                    .text_color(cx.theme().muted_foreground)
                    .child(Icon::new(IconName::Folder).size_4().flex_shrink_0())
                    .child(
                        v_flex()
                            .min_w_0()
                            .children(
                                self.path
                                    .clone()
                                    .map(|path| div().whitespace_normal().child(path)),
                            )
                            .child(self.host.clone()),
                    ),
            )
    }
}

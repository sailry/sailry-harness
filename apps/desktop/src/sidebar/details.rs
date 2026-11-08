use gpui_kit::{component::*, *};

pub(super) struct Details {
    pub title: SharedString,
    pub project: Option<SharedString>,
    pub branch: Option<SharedString>,
    pub path: Option<SharedString>,
}

impl Details {
    pub(super) fn render(&self, window: &Window, cx: &App) -> Div {
        let line = |icon: Icon, text: SharedString| {
            h_flex()
                .w_full()
                .min_w_0()
                .gap_1p5()
                .child(icon.size_3p5().flex_shrink_0())
                .child(div().flex_1().min_w_0().truncate().child(text))
        };
        v_flex()
            .w(px(320.).min(window.viewport_size().width - px(48.)))
            .min_w_0()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .font_medium()
                    .whitespace_normal()
                    .line_clamp(3)
                    .text_ellipsis()
                    .child(self.title.clone()),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .gap_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .children(
                        self.project
                            .clone()
                            .map(|project| line(Icon::new(IconName::Folder), project)),
                    )
                    .children(self.branch.clone().map(|branch| {
                        line(Icon::new(gpui_kit::assets::IconName::GitBranch), branch)
                    }))
                    .children(self.path.clone().map(|path| {
                        div()
                            .min_w_0()
                            .whitespace_normal()
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(path)
                    })),
            )
    }
}

use super::*;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    scroll::ScrollableElement,
    separator::Separator,
    spinner::Spinner,
    tab::{Tab, TabBar},
};
use gpui_kit::prelude::FluentBuilder as _;

impl Render for Assets {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Button::new("live-assets")
            .debug_selector(|| "live-assets".into())
            .ghost()
            .small()
            .icon(IconName::Folder)
            .disabled(self.session.is_none())
            .tooltip(tr("chat_assets_title"))
            .accessibility_label(tr("chat_assets_title"))
            .on_click(cx.listener(|panel, _, window, cx| panel.show(window, cx)))
    }
}

impl Assets {
    pub(super) fn content(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let selected = match self.kind {
            None => 0,
            Some(Kind::Resource) => 1,
            Some(Kind::Artifact) => 2,
        };
        let tabs = TabBar::new("asset-tabs")
            .segmented()
            .selected_index(selected)
            .children(
                [
                    "chat_assets_all",
                    "chat_assets_resources",
                    "chat_assets_artifacts",
                ]
                .into_iter()
                .enumerate()
                .map(|(index, key)| {
                    Tab::new()
                        .label(tr(key))
                        .debug_selector(move || format!("asset-tab-{index}"))
                }),
            )
            .on_click(cx.listener(|panel, index: &usize, window, cx| {
                let kind = match index {
                    1 => Some(Kind::Resource),
                    2 => Some(Kind::Artifact),
                    _ => None,
                };
                if kind != panel.kind {
                    panel.kind = kind;
                    panel.groups.clear();
                    panel.before = None;
                    panel.loaded_revision = None;
                    panel.load(false, window, cx);
                }
            }));
        // Kit's percentage-height wrapper clips this auto-growing dialog; use its native viewport.
        let mut body = v_flex()
            .id("asset-timeline")
            .relative()
            .gap_4()
            .w_full()
            .h_auto()
            .min_h_32()
            .max_h((window.viewport_size().height - px(240.)).max(px(120.)))
            .overflow_y_scroll()
            .track_scroll(&self.scroll);
        // Kit has no timeline control. Passive chronology decorates Kit action buttons;
        // the Stepper is reserved for progress/navigation, not historical events.
        for group in &self.groups {
            let stamp = chrono::DateTime::from_timestamp_millis(group.timestamp_ms)
                .map(|time| {
                    time.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M")
                        .to_string()
                })
                .unwrap_or_default();
            let mut entries = v_flex()
                .gap_2()
                .pl_4()
                .border_l_1()
                .border_color(cx.theme().border);
            for (index, target) in group.items.iter().enumerate() {
                let (name, icon) = match target {
                    Target::Attachment(value) => (value.spec.name.clone(), IconName::File),
                    Target::Image(value) => (value.attachment.spec.name.clone(), IconName::File),
                    Target::File { path } => (path.clone(), IconName::File),
                };
                let source = match target {
                    Target::Attachment(value) => Some(ImageSource::Attachment(value.clone())),
                    Target::Image(value) => self.session.map(|session| ImageSource::Image {
                        session,
                        image: value.clone(),
                    }),
                    Target::File { path } if crate::content::images::supports(path) => {
                        Some(ImageSource::File {
                            context: None,
                            worktree: group.worktree,
                            path: path.clone(),
                        })
                    }
                    _ => None,
                };
                let target = target.clone();
                let worktree = group.worktree;
                let key = format!("asset-{}-{index}", group.sequence);
                let selector = key.clone();
                let mut row = h_flex().w_full().gap_3().items_center();
                if let Some(source) = source.filter(|source| source.format().is_some()) {
                    row = row.child(Images::media(&self.images, source, cx).size_12());
                } else {
                    row = row.child(
                        Icon::new(icon)
                            .size_5()
                            .text_color(cx.theme().muted_foreground),
                    );
                }
                entries = entries.child(
                    Button::new(key)
                        .debug_selector(move || selector.clone())
                        .ghost()
                        .h_auto()
                        .w_full()
                        .p_3()
                        .justify_start()
                        .accessibility_label(name.clone())
                        .child(row.child(div().flex_1().min_w_0().truncate().text_sm().child(name)))
                        .on_click(cx.listener(move |panel, _, window, cx| {
                            panel.select(target.clone(), worktree, window, cx)
                        })),
                );
            }
            body = body.child(
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(stamp)
                            .child(tr(match group.kind {
                                Kind::Resource => "chat_assets_resources",
                                Kind::Artifact => "chat_assets_artifacts",
                            })),
                    )
                    .child(entries),
            );
        }
        if self.groups.is_empty() && !self.loading && !self.failed {
            body = body.child(
                div()
                    .h_32()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("chat_assets_empty")),
            );
        }
        if self.failed {
            let retry =
                v_flex().gap_2().items_center().child(
                    Button::new("asset-retry")
                        .label(tr("chat_retry"))
                        .debug_selector(|| "asset-retry".into())
                        .on_click(cx.listener(|panel, _, window, cx| {
                            panel.load(panel.retry_more, window, cx)
                        })),
                );
            #[cfg(test)]
            let retry = retry.on_children_prepainted(|bounds, window, cx| {
                if let Some(bounds) = bounds.first() {
                    tests::record_retry(*bounds, window, cx);
                }
            });
            body = body.child(retry);
        } else if self.before.is_some() && !self.loading {
            body = body.child(
                Button::new("asset-more")
                    .ghost()
                    .label(tr("chat_assets_more"))
                    .debug_selector(|| "asset-more".into())
                    .on_click(cx.listener(|panel, _, window, cx| panel.load(true, window, cx))),
            );
        }
        v_flex()
            .gap_4()
            .w_full()
            .debug_selector(|| "asset-panel".into())
            .child(
                h_flex().justify_between().child(tabs).child(
                    Button::new("asset-refresh")
                        .ghost()
                        .small()
                        .icon(IconName::RotateCw)
                        .tooltip(tr("chat_assets_refresh"))
                        .accessibility_label(tr("chat_assets_refresh"))
                        .disabled(self.loading)
                        .on_click(
                            cx.listener(|panel, _, window, cx| panel.load(false, window, cx)),
                        ),
                ),
            )
            .child(Separator::horizontal())
            .child(
                div()
                    .relative()
                    .child(body.vertical_scrollbar(&self.scroll))
                    .when(self.loading, |content| {
                        content.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(cx.theme().background.opacity(0.7))
                                .child(Spinner::new()),
                        )
                    }),
            )
            .into_any_element()
    }
}

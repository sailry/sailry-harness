use super::*;
use crate::conversation::subagent::Choice;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    popover::Popover,
    scroll::ScrollableElement,
    separator::Separator,
    spinner::Spinner,
};
use gpui_kit::prelude::FluentBuilder as _;

impl Render for Assets {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = cx.entity().downgrade();
        let session = self.session;
        let selector = session
            .map(|id| id.to_string())
            .unwrap_or_else(|| "draft".into());
        let popover = Popover::new("live-assets-popover")
            .anchor(Anchor::TopRight)
            .open(self.open)
            .p_3()
            .trigger(
                Button::new("live-assets")
                    .debug_selector(|| "live-assets".into())
                    .ghost()
                    .small()
                    .icon(Icon::new(gpui_kit::assets::IconName::ListTodo))
                    .disabled(session.is_none())
                    .tooltip(tr("chat_assets_title"))
                    .accessibility_label(tr("chat_assets_title")),
            )
            .on_open_change(
                cx.listener(|panel, open, window, cx| panel.set_open(*open, window, cx)),
            )
            .content(move |_, window, cx| {
                panel
                    .update(cx, |panel, cx| panel.content(window, cx))
                    .unwrap_or_else(|_| div().into_any_element())
            });
        div()
            .debug_selector(move || format!("live-overview-{selector}"))
            .child(popover)
    }
}

impl Assets {
    pub(super) fn content(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let overview = self
            .owner
            .upgrade()
            .map(|owner| overview::Overview::new(owner.read(cx)))
            .unwrap_or_default();
        let selector = self
            .session
            .map(|id| id.to_string())
            .unwrap_or_else(|| "draft".into());
        let mut body = v_flex()
            .id("asset-overview")
            .relative()
            .gap_4()
            .w_full()
            .max_h((window.viewport_size().height - px(128.)).max(px(120.)))
            .overflow_y_scroll()
            .track_scroll(&self.scroll);
        let count = overview.children.len();
        let mut activity = section("activity", "chat_assets_activity").child(
            h_flex()
                .debug_selector(|| "asset-activity-heading".into())
                .gap_2()
                .when(count > 0, |row| {
                    row.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                rust_i18n::t!("composer_subagent_count", count = count).to_string(),
                            ),
                    )
                }),
        );
        for child in overview.children {
            let Choice::Live(id) = child.key else {
                continue;
            };
            activity = activity.child(
                Button::new(format!("asset-child-{id}"))
                    .debug_selector(move || format!("asset-child-{id}"))
                    .ghost()
                    .w_full()
                    .h_auto()
                    .p_2()
                    .justify_start()
                    .accessibility_label(format!("{}: {}", child.name, tr(child.status)))
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .child(crate::ui::identicon::animated(
                                Icon::new(child.icon).size_4(),
                                child.running,
                            ))
                            .child(div().flex_1().min_w_0().truncate().child(child.name))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(tr(child.status)),
                            ),
                    )
                    .on_click(
                        cx.listener(move |panel, _, window, cx| panel.select_child(id, window, cx)),
                    ),
            );
        }
        if count == 0 {
            activity = activity.child(empty("chat_assets_no_activity", cx));
        }
        body = body.child(activity).child(Separator::horizontal());
        let mut progress = section("progress", "chat_assets_progress");
        if let Some(plan) = overview.progress {
            let (completed, total) = tools::progress::counts(&plan);
            progress = progress
                .child(
                    div()
                        .debug_selector(|| "asset-progress-heading".into())
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            rust_i18n::t!(
                                "composer_todo_count",
                                completed = completed,
                                total = total
                            )
                            .to_string(),
                        ),
                )
                .child(tools::progress::list(
                    &format!("overview-{}", overview.turn.unwrap()),
                    &plan,
                    false,
                    cx,
                ));
        } else {
            progress = progress.child(empty("chat_assets_no_progress", cx));
        }
        body = body.child(progress).child(Separator::horizontal());
        let mut output = section("output", "chat_assets_output");
        let mut sources = section("sources", "chat_assets_sources");
        let mut output_count = 0;
        let mut source_count = overview.sources.len();
        for group in &self.groups {
            for (index, target) in group.items.iter().enumerate() {
                let row = self.asset(group, index, target, cx);
                if group.kind == Kind::Artifact {
                    output = output.child(row);
                    output_count += 1;
                } else {
                    sources = sources.child(row);
                    source_count += 1;
                }
            }
        }
        for (index, source) in overview.sources.into_iter().enumerate() {
            let uri = source.uri;
            let label = source
                .title
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| uri.clone());
            sources = sources.child(
                Button::new(("asset-source", index))
                    .debug_selector(move || format!("asset-source-{index}"))
                    .ghost()
                    .w_full()
                    .h_auto()
                    .p_2()
                    .justify_start()
                    .accessibility_label(label.clone())
                    .icon(IconName::Globe)
                    .child(div().flex_1().min_w_0().truncate().text_left().child(label))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.select_source(uri.clone(), window, cx)
                    })),
            );
        }
        if output_count == 0 && !self.loading && !self.failed {
            output = output.child(empty("chat_assets_no_output", cx));
        }
        if source_count == 0 && !self.loading && !self.failed {
            sources = sources.child(empty("chat_assets_no_sources", cx));
        }
        body = body
            .child(output)
            .child(Separator::horizontal())
            .child(sources);
        if self.failed {
            let retry =
                v_flex().items_center().child(
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
        let panel = v_flex()
            .debug_selector(|| "asset-panel".into())
            .w(px(360.).min(window.viewport_size().width - px(48.)))
            .min_w_0()
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
                                .bg(cx.theme().popover.opacity(0.7))
                                .child(Spinner::new()),
                        )
                    }),
            );
        div()
            .debug_selector(move || format!("overview-{selector}"))
            .child(panel)
            .into_any_element()
    }

    fn asset(
        &self,
        group: &Group,
        index: usize,
        target: &Target,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = match target {
            Target::Attachment(value) => value.spec.name.clone(),
            Target::Image(value) => value.attachment.spec.name.clone(),
            Target::File { path } => path.clone(),
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
        let mut row = h_flex().w_full().min_w_0().gap_2();
        if let Some(source) = source.filter(|source| source.format().is_some()) {
            row = row.child(Images::media(&self.images, source, cx).size_8());
        } else {
            row = row.child(
                Icon::new(IconName::File)
                    .size_4()
                    .text_color(cx.theme().muted_foreground),
            );
        }
        Button::new(key)
            .debug_selector(move || selector.clone())
            .ghost()
            .w_full()
            .h_auto()
            .p_2()
            .justify_start()
            .accessibility_label(name.clone())
            .child(row.child(div().flex_1().min_w_0().truncate().text_left().child(name)))
            .on_click(cx.listener(move |panel, _, window, cx| {
                panel.select(target.clone(), worktree, window, cx)
            }))
            .into_any_element()
    }
}

fn section(id: &'static str, label: &'static str) -> Div {
    v_flex()
        .debug_selector(move || format!("asset-{id}"))
        .gap_1()
        .w_full()
        .min_w_0()
        .child(div().text_sm().font_semibold().child(tr(label)))
}

fn empty(label: &'static str, cx: &App) -> Div {
    div()
        .py_1()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(tr(label))
}

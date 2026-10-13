use super::*;
use gpui_kit::assets::IconName;
use gpui_kit::base::actions::Confirm;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IndexPath, Sizable as _, h_flex, tooltip::Tooltip, v_flex,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    list::{ListDelegate, ListItem},
    progress::Progress,
};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_protocol::installation::{Phase, Progress as Measurement};

pub(super) struct Row {
    key: Key,
    resource: Resource,
    status: Presentation,
    actionable: bool,
    connected: bool,
    retryable: bool,
}

pub(super) fn snapshot(entries: &BTreeMap<Key, Entry>) -> Vec<Row> {
    let mut rows: Vec<_> = entries
        .iter()
        .map(|(key, entry)| {
            (
                entry.order,
                Row {
                    key: *key,
                    resource: entry.resource.clone(),
                    status: entry.status.clone(),
                    actionable: entry.actionable(),
                    connected: entry.connected,
                    retryable: entry.retry.is_some(),
                },
            )
        })
        .collect();
    rows.sort_by_key(|(order, _)| *order);
    rows.into_iter().map(|(_, row)| row).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn preserves_measured_counts() {
        let progress = Measurement::Counts {
            completed: 3,
            total: 4,
            bytes_per_second: None,
        };
        assert_eq!(percentage(&progress), 75.);
        assert_eq!(measurement(&progress), "3/4");
        assert_eq!(
            percentage(&Measurement::Counts {
                completed: 0,
                total: 0,
                bytes_per_second: None
            }),
            0.
        );
    }
}

pub(super) struct Rows {
    pub owner: WeakEntity<Queue>,
    pub entries: Vec<Row>,
}

fn label(status: &Presentation) -> &'static str {
    match status {
        Presentation::Waiting => "installation_waiting",
        Presentation::Starting => "installation_starting",
        Presentation::Observed(Status::Queued) => "installation_queued",
        Presentation::Observed(Status::Failed { .. }) => "installation_failed",
        Presentation::Observed(Status::Uncertain { .. }) => "installation_uncertain",
        Presentation::Observed(Status::Running { phase, .. }) => match phase {
            Phase::Downloading => "installation_downloading",
            Phase::Inspecting => "installation_inspecting",
            Phase::Preparing => "installation_preparing",
            Phase::Installing => "installation_installing",
            Phase::Validating => "installation_validating",
        },
    }
}

pub(super) fn percentage(progress: &Measurement) -> f32 {
    match progress {
        Measurement::Percent { percent, .. } => f32::from(*percent),
        Measurement::Counts {
            completed, total, ..
        } if *total > 0 => (*completed as f64 / *total as f64 * 100.) as f32,
        Measurement::Counts { .. } => 0.,
    }
}

fn measurement(progress: &Measurement) -> String {
    let value = match progress {
        Measurement::Percent { percent, .. } => format!("{percent}%"),
        Measurement::Counts {
            completed, total, ..
        } => format!("{completed}/{total}"),
    };
    let speed = match progress {
        Measurement::Percent {
            bytes_per_second, ..
        }
        | Measurement::Counts {
            bytes_per_second, ..
        } => *bytes_per_second,
    };
    speed.map_or_else(
        || value.clone(),
        |speed| {
            let rate = rust_i18n::t!(
                "installation_speed",
                value = format!("{:.1}", speed as f64 / 1_048_576.)
            );
            format!("{value} · {rate}")
        },
    )
}

impl ListDelegate for Rows {
    type Item = ListItem;

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.entries.len()
    }

    fn set_selected_index(
        &mut self,
        _: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
    }

    fn render_item(
        &mut self,
        index: IndexPath,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let row = self.entries.get(index.row)?;
        let key = row.key;
        let title = match &row.resource {
            Resource::Plugin { name } => name.clone(),
            Resource::Model { id, .. } => id.clone(),
            Resource::Language { package, version } => format!("{package} {version}"),
        };
        let progress = match &row.status {
            Presentation::Observed(Status::Running { progress, .. }) => progress.as_ref(),
            _ => None,
        };
        let phase = if !row.connected && !row.status.terminal() {
            rust_i18n::t!("installation_offline", phase = tr(label(&row.status)))
                .to_string()
                .into()
        } else {
            tr(label(&row.status))
        };
        let location = tr(match row.key.0 {
            Location::Node(_) => "installation_node",
            Location::Controller => "installation_controller",
        });
        let status = progress.map_or_else(
            || format!("{phase} · {location}"),
            |progress| format!("{phase} · {location} · {}", measurement(progress)),
        );
        let retry = self.owner.clone();
        let retry_action = retry.clone();
        let remove = self.owner.clone();
        let remove_action = remove.clone();
        let spacing = cx.theme().spacing_tokens();
        let height = window.rem_size() * 2.75;
        let running = matches!(
            row.status,
            Presentation::Starting | Presentation::Observed(Status::Running { .. })
        );
        let (icon, tone, tile) = match &row.status {
            Presentation::Observed(Status::Failed { .. }) => (
                IconName::CircleAlert,
                cx.theme().danger,
                cx.theme().danger.opacity(0.1),
            ),
            Presentation::Observed(Status::Uncertain { .. }) => (
                IconName::CircleQuestionMark,
                cx.theme().warning,
                cx.theme().warning.opacity(0.1),
            ),
            _ => (
                match row.resource {
                    Resource::Plugin { .. } => IconName::Package,
                    Resource::Model { .. } => IconName::Cpu,
                    Resource::Language { .. } => IconName::Code,
                },
                cx.theme().foreground,
                cx.theme().muted,
            ),
        };
        let detail = match &row.status {
            Presentation::Observed(Status::Failed { error } | Status::Uncertain { error }) => {
                format!("{title}\n{status}\n{}", error.message)
            }
            _ => format!("{title}\n{status}"),
        };
        Some(
            ListItem::new(("installation", index.row))
                .h(height)
                .px_0()
                .py_0()
                .group("installation-row")
                .rounded(cx.theme().radius)
                .accessibility_label(detail.clone())
                .tooltip(move |window, cx| Tooltip::new(detail.clone()).build(window, cx))
                .debug_selector(move || format!("installation-{}", key.1))
                .child(
                    // Fill Kit's content wrapper and let the title column shrink
                    // so long labels cannot push the controls out.
                    h_flex()
                        .w_full()
                        .h(height)
                        .min_w_0()
                        .px(spacing.sm)
                        .gap(spacing.sm)
                        .debug_selector(move || format!("installation-row-{}", key.1))
                        .child(
                            div()
                                .size_7()
                                .flex_shrink_0()
                                .rounded(cx.theme().radius)
                                .bg(tile)
                                .flex()
                                .items_center()
                                .justify_center()
                                .debug_selector(move || format!("installation-icon-{}", key.1))
                                .child(Icon::new(icon).size_4().text_color(tone)),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .child(
                                    div()
                                        .id(("installation-title", index.row))
                                        .test_support()
                                        .role(Role::Label)
                                        .aria_label(title.clone())
                                        .w_full()
                                        .min_w_0()
                                        .truncate()
                                        .debug_selector(move || {
                                            format!("installation-title-{}", key.1)
                                        })
                                        .child(title),
                                )
                                .when(running, |view| {
                                    view.child(
                                        div()
                                            .w_full()
                                            .debug_selector(move || {
                                                format!("installation-progress-{}", key.1)
                                            })
                                            .child(
                                                Progress::new(("installation-progress", index.row))
                                                    .with_size(px(2.))
                                                    .color(cx.theme().foreground)
                                                    .loading(progress.is_none_or(|progress| {
                                                        matches!(
                                                            progress,
                                                            Measurement::Counts { total: 0, .. }
                                                        )
                                                    }))
                                                    .value(progress.map_or(0., percentage))
                                                    .accessibility_label(status.clone()),
                                            ),
                                    )
                                }),
                        )
                        .when_some(progress, |view, progress| {
                            view.child(
                                div()
                                    .id(("installation-measurement", index.row))
                                    .test_support()
                                    .role(Role::Label)
                                    .aria_label(measurement(progress))
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .debug_selector(move || {
                                        format!("installation-measurement-{}", key.1)
                                    })
                                    .child(match progress {
                                        Measurement::Percent { percent, .. } => {
                                            format!("{percent}%")
                                        }
                                        Measurement::Counts {
                                            completed, total, ..
                                        } => format!("{completed}/{total}"),
                                    }),
                            )
                        })
                        .when(
                            matches!(
                                row.status,
                                Presentation::Waiting | Presentation::Observed(Status::Queued)
                            ),
                            |view| {
                                view.child(
                                    div()
                                        .flex_shrink_0()
                                        .px_1()
                                        .debug_selector(move || {
                                            format!("installation-queued-{}", key.1)
                                        })
                                        .child(
                                            Icon::new(IconName::Clock)
                                                .size_4()
                                                .text_color(cx.theme().muted_foreground),
                                        ),
                                )
                            },
                        )
                        .when(row.actionable, |view| {
                            // Popover maps Enter and Space to Confirm before a
                            // Button can observe their raw key-down events.
                            view.child(
                                h_flex()
                                    .flex_shrink_0()
                                    .gap_1()
                                    .debug_selector(move || {
                                        format!("installation-actions-{}", key.1)
                                    })
                                    .when(row.retryable, |view| {
                                        view.child(
                                            Button::new(("installation-retry", index.row))
                                                .ghost()
                                                .xsmall()
                                                .icon(IconName::Redo)
                                                .text_color(tone)
                                                .tooltip(tr("installation_retry"))
                                                .accessibility_label(tr("installation_retry"))
                                                .debug_selector(move || {
                                                    format!("installation-retry-{}", key.1)
                                                })
                                                .on_action(move |_: &Confirm, window, cx| {
                                                    let _ = retry_action.update(cx, |queue, cx| {
                                                        queue.retry(key, window, cx)
                                                    });
                                                })
                                                .on_click(move |_, window, cx| {
                                                    let _ = retry.update(cx, |queue, cx| {
                                                        queue.retry(key, window, cx)
                                                    });
                                                }),
                                        )
                                    })
                                    .child(
                                        Button::new(("installation-remove", index.row))
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::Close)
                                            .opacity(0.)
                                            .group_hover("installation-row", |style| {
                                                style.opacity(1.)
                                            })
                                            .focus(|style| style.opacity(1.))
                                            .tooltip(tr("installation_remove"))
                                            .accessibility_label(tr("installation_remove"))
                                            .debug_selector(move || {
                                                format!("installation-remove-{}", key.1)
                                            })
                                            .on_action(move |_: &Confirm, window, cx| {
                                                let _ = remove_action.update(cx, |queue, cx| {
                                                    queue.remove(key, window, cx)
                                                });
                                            })
                                            .on_click(move |_, window, cx| {
                                                let _ = remove.update(cx, |queue, cx| {
                                                    queue.remove(key, window, cx)
                                                });
                                            }),
                                    ),
                            )
                        }),
                ),
        )
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let _ = self.owner.update(cx, |queue, cx| queue.hide(window, cx));
    }
}

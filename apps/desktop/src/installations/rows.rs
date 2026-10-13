use super::*;
use gpui_kit::component::{ActiveTheme as _, IconName, IndexPath, Sizable as _, h_flex, v_flex};
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

fn percentage(progress: &Measurement) -> f32 {
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
        _: &mut Window,
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
        let remove = self.owner.clone();
        Some(
            ListItem::new(("installation", index.row))
                .h(rems(3.5))
                .px_0()
                .py_0()
                .hover_background(false)
                .debug_selector(move || format!("installation-card-{}", key.1))
                .child(
                    // Kit's content wrapper has an intrinsic minimum. Anchor the
                    // columns to the row so long labels cannot push controls out.
                    h_flex()
                        .absolute()
                        .inset_0()
                        .min_w_0()
                        .px_2()
                        .gap_3()
                        .debug_selector(move || format!("installation-{}", key.1))
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
                                .child(
                                    div()
                                        .id(("installation-status", index.row))
                                        .test_support()
                                        .role(Role::Label)
                                        .aria_label(status.clone())
                                        .w_full()
                                        .min_w_0()
                                        .truncate()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .debug_selector(move || {
                                            format!("installation-status-{}", key.1)
                                        })
                                        .child(status),
                                ),
                        )
                        .when(!row.status.terminal(), |view| {
                            view.child(
                                div()
                                    .w_24()
                                    .flex_shrink_0()
                                    .debug_selector(move || {
                                        format!("installation-progress-{}", key.1)
                                    })
                                    .child(
                                        Progress::new(("installation-progress", index.row))
                                            .small()
                                            .loading(progress.is_none_or(|progress| {
                                                matches!(
                                                    progress,
                                                    Measurement::Counts { total: 0, .. }
                                                )
                                            }))
                                            .value(progress.map_or(0., percentage))
                                            .accessibility_label(phase),
                                    ),
                            )
                        })
                        .when(row.actionable, |view| {
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
                                                .small()
                                                .icon(IconName::Redo)
                                                .tooltip(tr("installation_retry"))
                                                .accessibility_label(tr("installation_retry"))
                                                .debug_selector(move || {
                                                    format!("installation-retry-{}", key.1)
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
                                            .small()
                                            .icon(IconName::Close)
                                            .tooltip(tr("installation_remove"))
                                            .accessibility_label(tr("installation_remove"))
                                            .debug_selector(move || {
                                                format!("installation-remove-{}", key.1)
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

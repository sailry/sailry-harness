use super::*;
use gpui_kit::component::switch::Switch;
use sailry_link::CancellationToken;
use sailry_node_runtime::logging::{Area, Entry, Phase, Span};
use sailry_protocol::{Command, ErrorCode, Output, conversation::catalog};

#[derive(Clone, Copy)]
pub(in crate::settings) enum Source {
    Automatic,
    Manual,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Manual => "manual",
        }
    }
}

pub(in crate::settings) struct Failure {
    key: &'static str,
    source: Source,
}

#[derive(Default)]
pub(in crate::settings) struct State {
    pub status: catalog::Status,
    pub refresh: Option<Refresh>,
    pub error: Option<Failure>,
    scores_error: Option<Failure>,
    toggle: Option<Refresh>,
}

impl State {
    pub(in crate::settings) fn feedback(&self) -> Option<&'static str> {
        self.error
            .as_ref()
            .and_then(|failure| matches!(failure.source, Source::Manual).then_some(failure.key))
    }

    pub(in crate::settings) fn scores_feedback(&self) -> Option<&'static str> {
        self.scores_error
            .as_ref()
            .and_then(|failure| matches!(failure.source, Source::Manual).then_some(failure.key))
    }

    pub(super) fn observe(&mut self, mut status: catalog::Status) {
        if status.scores.revision < self.status.scores.revision
            || status.scores.revision == self.status.scores.revision
                && status.scores.checked_at_ms < self.status.scores.checked_at_ms
        {
            status.scores = self.status.scores.clone();
        }
        if status.revision >= self.status.revision {
            self.status = status;
        } else {
            self.status.scores = status.scores;
        }
    }

    fn complete(&mut self, source: Source, result: Result<catalog::Status, ErrorCode>) {
        match result {
            Ok(status) => {
                self.error = status.fault.as_ref().map(|_| Failure {
                    key: "provider_catalog_failed",
                    source,
                });
                self.scores_error = status
                    .scores
                    .fault
                    .as_ref()
                    .filter(|_| status.scores.enabled)
                    .map(|_| Failure {
                        key: "provider_scores_failed",
                        source,
                    });
                self.observe(status);
            }
            Err(code) => {
                self.error = Some(Failure {
                    key: if code == ErrorCode::Busy {
                        "provider_catalog_busy"
                    } else {
                        "provider_catalog_failed"
                    },
                    source,
                });
            }
        }
    }
}

pub(in crate::settings) struct Refresh {
    stop: CancellationToken,
    _task: Task<()>,
}

impl Drop for Refresh {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Workspace {
    pub(in crate::settings) fn refresh_catalog(&mut self, source: Source, cx: &mut Context<Self>) {
        let Some(live) = &mut self.provider_link else {
            self.providers.catalog_revision += 1;
            cx.notify();
            return;
        };
        if !live.connected || live.catalog.refresh.is_some() {
            return;
        }
        let binding = live.binding.clone();
        let node = binding.client.target();
        let stop = CancellationToken::new();
        let cancelled = stop.clone();
        let request = binding.client.prepare(Command::RefreshModelCatalog);
        let mut entry = Entry::new(Area::Desktop, Phase::Begin);
        entry.operation = Some("model_catalog");
        entry.stage = Some("refresh");
        entry.source_kind = Some(source.name());
        entry.context.node = Some(node);
        entry.context.request = Some(request.id);
        let mut diagnostic = Span::new(entry).cancel_when(stop.clone());
        let job = binding.runtime.spawn(async move {
            let result = tokio::select! {
                biased;
                _ = cancelled.cancelled() => None,
                result = binding.client.execute(request) => Some(result),
            };
            match &result {
                Some(Ok(Output::CatalogStatus(status))) => match status.fault.as_ref().or(status
                    .scores
                    .fault
                    .as_ref()
                    .filter(|_| status.scores.enabled))
                {
                    Some(fault) => diagnostic.finish(Phase::Failed, Some(fault.code)),
                    None => diagnostic.finish(Phase::Completed, None),
                },
                Some(Err(error)) => diagnostic.finish(Phase::Failed, Some(error.code)),
                None => diagnostic.finish(Phase::Cancelled, Some(ErrorCode::Cancelled)),
                _ => diagnostic.finish(Phase::Failed, Some(ErrorCode::Internal)),
            };
            result
        });
        live.catalog.error = None;
        live.catalog.scores_error = None;
        live.catalog.refresh = Some(Refresh {
            stop,
            _task: cx.spawn(async move |owner, cx| {
                let result = job.await;
                let _ = owner.update(cx, |owner, cx| {
                    let Some(live) = &mut owner.provider_link else {
                        return;
                    };
                    if live.binding.client.target() != node {
                        return;
                    }
                    live.catalog.refresh = None;
                    let result = match result {
                        Ok(Some(Ok(Output::CatalogStatus(status)))) => Ok(status),
                        Ok(Some(Err(error))) => Err(error.code),
                        _ => Err(ErrorCode::Internal),
                    };
                    live.catalog.complete(source, result);
                    cx.notify();
                });
            }),
        });
        cx.notify();
    }

    fn set_scores(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let Some(live) = &mut self.provider_link else {
            self.providers.scores_enabled = enabled;
            cx.notify();
            return;
        };
        if !live.connected || live.catalog.toggle.is_some() {
            return;
        }
        let binding = live.binding.clone();
        let node = binding.client.target();
        let stop = CancellationToken::new();
        let cancelled = stop.clone();
        let job = binding.runtime.spawn(async move {
            tokio::select! {
                _ = cancelled.cancelled() => None,
                result = binding.client.execute(binding.client.prepare(Command::SetModelScoresEnabled { enabled })) => Some(result),
            }
        });
        live.catalog.scores_error = None;
        live.catalog.toggle = Some(Refresh {
            stop,
            _task: cx.spawn(async move |owner, cx| {
                let result = job.await;
                let _ = owner.update(cx, |owner, cx| {
                    let Some(live) = owner
                        .provider_link
                        .as_mut()
                        .filter(|live| live.binding.client.target() == node)
                    else {
                        return;
                    };
                    live.catalog.toggle = None;
                    match result {
                        Ok(Some(Ok(Output::CatalogStatus(status)))) => live.catalog.observe(status),
                        _ => {
                            live.catalog.scores_error = Some(Failure {
                                key: "provider_scores_failed",
                                source: Source::Manual,
                            })
                        }
                    }
                    cx.notify();
                });
            }),
        });
        cx.notify();
    }

    pub(super) fn model_catalog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let compaction = self.compaction_settings(window, cx);
        let live = self.provider_link.as_ref();
        let loading = live.is_some_and(|live| live.catalog.refresh.is_some());
        let preview = self.providers.catalog_revision > 0;
        let scores = live.map(|live| &live.catalog.status.scores);
        let scores_enabled = scores.map_or(self.providers.scores_enabled, |status| status.enabled);
        let scores_updated = scores.is_some_and(|status| status.updated_at_ms.is_some());
        let scores_source = scores
            .and_then(|status| status.published_at.as_deref())
            .map(crate::model_picker::score_date)
            .map(|date| rust_i18n::t!("provider_scores_source", date = date).to_string());
        let updated = live
            .map(|live| live.catalog.status.revision > 0)
            .unwrap_or(preview);
        let updated_at = if let Some(live) = live {
            live.catalog
                .status
                .updated_at_ms
                .and_then(|value| i64::try_from(value).ok())
                .and_then(chrono::DateTime::from_timestamp_millis)
                .map(|date| {
                    date.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M")
                        .to_string()
                })
                .unwrap_or_else(|| tr("provider_catalog_never").to_string())
        } else {
            tr(if preview {
                "provider_catalog_recent"
            } else {
                "provider_catalog_never"
            })
            .to_string()
        };
        Group::new("provider_catalog")
            .action(
                Button::new("models-dev-refresh")
                    .primary()
                    .disabled(loading || live.is_some_and(|live| !live.connected))
                    .debug_selector(|| "models-dev-refresh".into())
                    .label(tr("provider_catalog_update"))
                    .on_click(
                        cx.listener(|owner, _, _, cx| owner.refresh_catalog(Source::Manual, cx)),
                    ),
            )
            .child(Row::new(
                "provider_catalog_status",
                div()
                    .debug_selector(move || {
                        if updated {
                            "catalog-updated"
                        } else {
                            "catalog-empty"
                        }
                        .into()
                    })
                    .text_color(if updated {
                        cx.theme().success
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(tr(if updated {
                        "provider_catalog_updated"
                    } else {
                        "provider_catalog_never"
                    })),
            ))
            .child(Row::new(
                "provider_scores",
                h_flex()
                    .gap_3()
                    .child(div().text_color(cx.theme().muted_foreground).child(tr(
                        if live.is_none() {
                            "model_score_preview"
                        } else if scores_updated {
                            "provider_catalog_updated"
                        } else {
                            "provider_catalog_never"
                        },
                    )))
                    .when_some(scores_source, |row, source| {
                        row.child(
                            Button::new("model-scores-source")
                                .text()
                                .small()
                                .label(source)
                                .on_click(|_, _, cx| {
                                    cx.open_url(crate::model_picker::SCORE_SOURCE)
                                }),
                        )
                    })
                    .child(
                        Switch::new("model-scores-enabled")
                            .accessibility_label(tr("provider_scores"))
                            .checked(scores_enabled)
                            .disabled(live.is_some_and(|live| {
                                !live.connected || live.catalog.toggle.is_some()
                            }))
                            .on_click(cx.listener(|owner, enabled: &bool, _, cx| {
                                owner.set_scores(*enabled, cx)
                            })),
                    ),
            ))
            .child(Row::new("provider_catalog_last_update", updated_at))
            .child(compaction)
    }
}

#[cfg(test)]
mod tests;

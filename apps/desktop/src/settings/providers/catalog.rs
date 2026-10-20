use super::*;
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
}

impl State {
    pub(in crate::settings) fn feedback(&self) -> Option<&'static str> {
        self.error
            .as_ref()
            .and_then(|failure| matches!(failure.source, Source::Manual).then_some(failure.key))
    }

    fn complete(&mut self, source: Source, result: Result<catalog::Status, ErrorCode>) {
        match result {
            Ok(status) => {
                if status.revision >= self.status.revision {
                    self.status = status;
                }
                self.error = None;
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
                Some(Ok(Output::CatalogStatus(_))) => diagnostic.finish(Phase::Completed, None),
                Some(Err(error)) => diagnostic.finish(Phase::Failed, Some(error.code)),
                None => diagnostic.finish(Phase::Cancelled, Some(ErrorCode::Cancelled)),
                _ => diagnostic.finish(Phase::Failed, Some(ErrorCode::Internal)),
            };
            result
        });
        live.catalog.error = None;
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

    pub(super) fn model_catalog(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let live = self.provider_link.as_ref();
        let loading = live.is_some_and(|live| live.catalog.refresh.is_some());
        let preview = self.providers.catalog_revision > 0;
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
            .when(
                live.is_some_and(|live| live.catalog.error.is_some()),
                |group| {
                    group.action(
                        Button::new("models-dev-refresh")
                            .primary()
                            .disabled(loading || live.is_some_and(|live| !live.connected))
                            .debug_selector(|| "models-dev-refresh".into())
                            .label(tr("provider_catalog_update"))
                            .on_click(cx.listener(|owner, _, _, cx| {
                                owner.refresh_catalog(Source::Manual, cx)
                            })),
                    )
                },
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
            .child(Row::new("provider_catalog_last_update", updated_at))
    }
}

#[cfg(test)]
mod tests;

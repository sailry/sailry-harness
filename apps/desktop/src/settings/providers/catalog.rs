use super::*;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, ErrorCode, Output, conversation::catalog};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(in crate::settings) struct State {
    pub status: catalog::Status,
    pub refresh: Option<Refresh>,
    pub error: Option<&'static str>,
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
    pub(in crate::settings) fn refresh_catalog(&mut self, cx: &mut Context<Self>) {
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
        let job = binding.runtime.spawn(async move {
            tokio::select! {
                biased;
                _ = cancelled.cancelled() => None,
                result = binding.client.execute(binding.client.prepare(Command::RefreshModelCatalog)) => Some(result),
            }
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
                    match result {
                        Ok(Some(Ok(Output::CatalogStatus(status)))) => {
                            if status.revision >= live.catalog.status.revision {
                                live.catalog.status = status;
                            }
                        }
                        Ok(Some(Err(error))) if error.code == ErrorCode::Busy => {
                            live.catalog.error = Some("provider_catalog_busy");
                        }
                        _ => live.catalog.error = Some("provider_catalog_failed"),
                    }
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
                            .on_click(cx.listener(|owner, _, _, cx| owner.refresh_catalog(cx))),
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

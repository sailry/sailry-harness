//! Controller-local theme checks and publication share the existing acquisition service.
use super::*;
use crate::theme::market as service;
use sailry_link::CancellationToken;
use std::{collections::BTreeMap, sync::Arc};

pub(in crate::settings) struct State {
    pub(in crate::settings) service: Option<Arc<service::Service>>,
    available: BTreeMap<String, service::Selection>,
    pending: BTreeMap<String, CancellationToken>,
    checking: Option<CancellationToken>,
    task: Option<Task<()>>,
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(stop) = &self.checking {
            stop.cancel();
        }
        for stop in self.pending.values() {
            stop.cancel();
        }
    }
}

impl State {
    pub(in crate::settings) fn new(cx: &App) -> Self {
        Self {
            service: cx
                .global::<theme::Catalog>()
                .root
                .clone()
                .and_then(|root| service::Service::new(root).ok())
                .map(Arc::new),
            available: BTreeMap::new(),
            pending: BTreeMap::new(),
            checking: None,
            task: None,
        }
    }

    pub(super) fn checking(&self) -> bool {
        self.checking.is_some()
    }
    pub(super) fn available(&self, id: &str) -> bool {
        self.available.contains_key(id)
    }
    pub(in crate::settings) fn pending(&self, id: &str) -> bool {
        self.pending.contains_key(id)
    }

    pub(super) fn check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.checking.is_some() {
            return;
        }
        let Some(service) = self.service.clone() else {
            return;
        };
        let ids: Vec<_> = cx
            .global::<theme::Catalog>()
            .packages
            .iter()
            .filter(|package| package.manifest.is_some())
            .map(|package| package.id.clone())
            .collect();
        let stop = CancellationToken::new();
        self.checking = Some(stop.clone());
        let cancelled = stop.clone();
        let runtime = cx.global::<crate::backend::Services>().runtime.clone();
        let job = runtime.spawn(async move {
            let inspections = futures::future::join_all(ids.into_iter().map(|id| {
                let service = service.clone();
                async move {
                    service
                        .installed(&id)
                        .await
                        .map(|installed| (id, installed))
                }
            }));
            let (listing, installed) = tokio::join!(service.list(true, &cancelled), inspections);
            let listing = listing?;
            if listing.unavailable {
                return Err(service::Failure::Invalid);
            }
            let mut available = BTreeMap::new();
            for result in installed {
                let (id, installed) = result?;
                if let Some(installed) = installed
                    && let Some(selection) = listing.selection(&id)
                    && selection.entry.update_available(&installed)
                {
                    available.insert(id, selection);
                }
            }
            Ok(available)
        });
        self.task = Some(cx.spawn_in(window, async move |state, cx| {
            let result = job.await;
            _ = state.update_in(cx, |state, window, cx| {
                if stop.is_cancelled() {
                    return;
                }
                state.checking = None;
                match result {
                    Ok(Ok(available)) => {
                        state.available = available;
                        feedback(
                            if state.available.is_empty() {
                                "appearance_up_to_date"
                            } else {
                                "appearance_updates_available"
                            },
                            false,
                            window,
                            cx,
                        );
                    }
                    _ => feedback("market_themes_failed", true, window, cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn update(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.available.get(id).cloned() else {
            return;
        };
        if !cx
            .global::<theme::Catalog>()
            .packages
            .iter()
            .any(|package| package.id == id)
        {
            return;
        }
        self.acquire(selection, true, window, cx);
    }

    pub(in crate::settings) fn install(
        &mut self,
        selection: service::Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if cx
            .global::<theme::Catalog>()
            .packages
            .iter()
            .any(|package| package.id == selection.entry.id)
        {
            feedback("appearance_import_conflict", true, window, cx);
            return;
        }
        self.acquire(selection, false, window, cx);
    }

    fn acquire(
        &mut self,
        selection: service::Selection,
        replacing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = selection.entry.id.clone();
        if self.pending.contains_key(&id) {
            return;
        }
        let Some(service) = self.service.clone() else {
            return;
        };
        let stop = CancellationToken::new();
        self.pending.insert(id.clone(), stop.clone());
        let cancelled = stop.clone();
        let downloader = service.clone();
        let runtime = cx.global::<crate::backend::Services>().runtime.clone();
        let job = runtime.spawn(async move { downloader.download(selection, &cancelled).await });
        let origin = window.window_handle();
        cx.spawn(async move |state, cx| {
            let result = job.await;
            let prepared = cx.update(|cx| {
                if stop.is_cancelled() {
                    return None;
                }
                match result {
                    Ok(Ok(prepared)) => {
                        let catalog = cx.global::<theme::Catalog>();
                        if (!replacing && catalog.packages.iter().any(|package| package.id == id))
                            || catalog.check(&prepared.loaded).is_err()
                        {
                            finish(&state, &id, "appearance_import_conflict", true, origin, cx);
                            None
                        } else {
                            Some(prepared)
                        }
                    }
                    _ => {
                        finish(&state, &id, "appearance_load_failed", true, origin, cx);
                        None
                    }
                }
            });
            let Some(prepared) = prepared else {
                return;
            };
            // Publication must update the controller Catalog even after this view closes.
            let job = runtime.spawn(async move { service.install(prepared, &stop).await });
            let result = job.await;
            cx.update(|cx| {
                let outcome = match result {
                    Ok(Ok(result)) => {
                        let registered = cx.update_global::<theme::Catalog, _>(|catalog, _| {
                            let active = catalog.packages[catalog.selected].id == id;
                            catalog.add(result.loaded)?;
                            catalog.error = None;
                            Ok::<_, String>(
                                active.then(|| catalog.packages[catalog.selected].clone()),
                            )
                        });
                        match registered {
                            Ok(active) => {
                                _ = state.update(cx, |state, _| {
                                    state.available.retain(|name, selection| {
                                        name != &id
                                            || selection.entry.update_available(&result.installed)
                                    });
                                });
                                if let Some(package) = active {
                                    theme::apply_colors(package, None, cx);
                                }
                                cx.refresh_windows();
                                Some(("appearance_import_done", false))
                            }
                            Err(_) => Some(("appearance_load_failed", true)),
                        }
                    }
                    Ok(Err(service::Failure::Cancelled)) => None,
                    Ok(Err(service::Failure::Publication { .. })) | Err(_) => {
                        Some(("appearance_save_uncertain", true))
                    }
                    Ok(Err(_)) => Some(("appearance_save_failed", true)),
                };
                if let Some((key, failed)) = outcome {
                    finish(&state, &id, key, failed, origin, cx);
                } else {
                    _ = state.update(cx, |state, cx| {
                        state.pending.remove(&id);
                        cx.notify();
                    });
                }
            });
        })
        .detach();
        cx.notify();
    }
}

fn finish(
    state: &WeakEntity<State>,
    id: &str,
    key: &'static str,
    failed: bool,
    origin: AnyWindowHandle,
    cx: &mut App,
) {
    _ = state.update(cx, |state, cx| {
        state.pending.remove(id);
        cx.notify();
    });
    if let Some(target) = crate::installations::target(origin, cx) {
        _ = target.update(cx, |_, window, cx| feedback(key, failed, window, cx));
    }
}

fn feedback(key: &'static str, failed: bool, window: &mut Window, cx: &mut App) {
    let message = tr(key);
    let notification = if failed {
        notification::Notification::error(message.clone())
    } else {
        notification::Notification::success(message.clone())
    };
    crate::feedback::toast(window, message, notification, cx);
}

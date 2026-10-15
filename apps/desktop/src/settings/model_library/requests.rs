use super::*;
use sailry_link::CancellationToken;
use sailry_protocol::{Fault, Output, models::Search};

pub(super) struct Read {
    stop: CancellationToken,
    _task: Task<()>,
}

impl Drop for Read {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Library {
    pub(super) fn has_more(&self, cx: &App) -> bool {
        !self.loading
            && self.repository.is_none()
            && self.available(cx)
            && self
                .cursor
                .as_ref()
                .is_some_and(|cursor| !self.pages.contains(cursor))
            && self.searched.as_ref()
                == Some(&(self.hub(cx), self.query.read(cx).value().trim().to_owned()))
    }

    pub(super) fn cancel(&mut self) {
        self.read = None;
        self.generation += 1;
        self.loading = false;
    }

    fn request(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
        apply: impl FnOnce(&mut Self, Result<Output, Fault>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        self.cancel();
        self.loading = true;
        let generation = self.generation;
        let stop = CancellationToken::new();
        let cancelled = stop.clone();
        let binding = self.binding.clone();
        // Only catalog reads wait here; admitted downloads belong to the target panel.
        let job = binding.runtime.spawn(async move {
            tokio::select! {
                biased;
                _ = cancelled.cancelled() => None,
                result = binding.client.execute(binding.client.prepare(command)) => Some(result),
            }
        });
        self.read = Some(Read {
            stop: stop.clone(),
            _task: cx.spawn_in(window, async move |owner, cx| {
                let result = job.await;
                if stop.is_cancelled() {
                    return;
                }
                _ = owner.update_in(cx, |library, window, cx| {
                    if library.generation != generation || stop.is_cancelled() {
                        return;
                    }
                    library.read = None;
                    library.loading = false;
                    if !library.available(cx) {
                        cx.notify();
                        return;
                    }
                    match result {
                        Ok(Some(result)) => apply(library, result, window, cx),
                        Ok(None) => (),
                        Err(_) => library.failure(window, cx),
                    }
                    cx.notify();
                });
            }),
        });
        cx.notify();
    }

    pub(super) fn search(&mut self, more: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.search_hub(self.hub(cx), more, window, cx);
    }

    pub(super) fn search_hub(
        &mut self,
        hub: Hub,
        more: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if (more && self.loading) || !self.available(cx) {
            return;
        }
        let query = self.query.read(cx).value().trim().to_owned();
        let cursor = if more { self.cursor.clone() } else { None };
        if more {
            if !self.has_more(cx) {
                return;
            }
            self.pages
                .insert(cursor.clone().expect("pagination cursor is present"));
        }
        self.installing = None;
        let searched = (hub, query.clone());
        let kind = self.target.kind();
        self.request(
            Command::SearchModels(Search {
                hub,
                kind,
                query,
                cursor,
            }),
            window,
            cx,
            move |library, result, window, cx| match result {
                Ok(Output::ModelSearch(page)) => {
                    if !more {
                        library.items.clear();
                        library.repository = None;
                        library.definition = None;
                        library.pages.clear();
                        library
                            .version
                            .update(cx, |state, cx| state.set_items(Vec::new(), window, cx));
                    }
                    for item in page.items {
                        if !library
                            .items
                            .iter()
                            .any(|previous| previous.source == item.source)
                        {
                            library.items.push(item);
                        }
                    }
                    library.cursor = page.next_cursor;
                    library.hub.update(cx, |state, cx| {
                        state.set_selected_value(&searched.0, window, cx)
                    });
                    library.searched = Some(searched);
                    if page.partial {
                        crate::feedback::info("", tr("model_library_partial").as_ref(), window, cx);
                    }
                    // Kit only checks the visible range when a list has rows.
                    // Empty catalog pages can still carry a valid continuation.
                    if library.items.is_empty() && library.has_more(cx) {
                        cx.defer_in(window, |library, window, cx| {
                            library.search(true, window, cx)
                        });
                    }
                }
                _ => library.failure(window, cx),
            },
        );
    }

    pub(super) fn install(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.available(cx) || self.target.busy(cx) {
            return;
        }
        let Some(item) = self.items.get(index) else {
            return;
        };
        if matches!(
            self.target.source_status(&item.source, cx),
            ModelStatus::Installed | ModelStatus::Downloading(_) | ModelStatus::Checking
        ) {
            return;
        }
        let expected = item.source.clone();
        self.installing = Some(expected.clone());
        let kind = self.target.kind();
        self.repository = Some(expected.repository.clone());
        self.definition = None;
        self.request(
            Command::ResolveModel {
                kind,
                source: expected.clone(),
            },
            window,
            cx,
            move |library, result, window, cx| {
                library.repository = None;
                match result {
                    Ok(Output::ModelDefinition(definition))
                        if definition.format.kind() == kind
                            && definition.source.hub == expected.hub
                            && definition.source.repository == expected.repository
                            && definition.source.revision == expected.revision
                            && definition.validate().is_ok() =>
                    {
                        if !matches!(
                            library.target.model_status(&definition, cx),
                            ModelStatus::Installed
                                | ModelStatus::Downloading(_)
                                | ModelStatus::Checking
                        ) {
                            library.target.download(definition, window, cx);
                        }
                    }
                    _ => {
                        library.installing = None;
                        library.failure(window, cx);
                    }
                }
            },
        );
    }

    pub(super) fn versions(
        &mut self,
        repository: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.available(cx) {
            return;
        }
        self.installing = None;
        self.repository = Some(repository.clone());
        self.definition = None;
        self.request(
            Command::ReadModelVersions {
                hub: self.hub(cx),
                repository,
            },
            window,
            cx,
            |library, result, window, cx| match result {
                Ok(Output::ModelVersions(versions)) if !versions.is_empty() => {
                    let selected = versions[0].revision.clone();
                    library.version.update(cx, |state, cx| {
                        state.set_items(
                            versions.into_iter().map(VersionChoice).collect(),
                            window,
                            cx,
                        );
                        state.set_selected_value(&selected, window, cx);
                    });
                    library.resolve(window, cx);
                }
                _ => library.failure(window, cx),
            },
        );
    }

    pub(super) fn resolve(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.available(cx) {
            return;
        }
        let Some(revision) = self.version.read(cx).selected_value().cloned() else {
            return;
        };
        self.resolve_revision(revision, window, cx);
    }

    pub(super) fn resolve_revision(
        &mut self,
        revision: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.available(cx) {
            return;
        }
        let Some(repository) = self.repository.clone() else {
            return;
        };
        let expected = Source {
            hub: self.hub(cx),
            repository,
            revision,
        };
        self.request(
            Command::ResolveModel {
                kind: self.target.kind(),
                source: expected.clone(),
            },
            window,
            cx,
            move |library, result, window, cx| match result {
                Ok(Output::ModelDefinition(definition))
                    if definition.format.kind() == library.target.kind()
                        && definition.source.hub == expected.hub
                        && definition.source.repository == expected.repository
                        && definition.source.revision == expected.revision
                        && definition.validate().is_ok() =>
                {
                    library.version.update(cx, |state, cx| {
                        state.set_selected_value(&expected.revision, window, cx)
                    });
                    library.query_prefix.update(cx, |state, cx| {
                        state.set_value(
                            definition.query_prefix.clone().unwrap_or_default(),
                            window,
                            cx,
                        )
                    });
                    library.document_prefix.update(cx, |state, cx| {
                        state.set_value(
                            definition.document_prefix.clone().unwrap_or_default(),
                            window,
                            cx,
                        )
                    });
                    library.definition = Some(definition)
                }
                _ => library.failure(window, cx),
            },
        );
    }
}

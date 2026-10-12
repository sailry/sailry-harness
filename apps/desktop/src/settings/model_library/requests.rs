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
        if self.loading || !self.available(cx) {
            return;
        }
        let query = self.query.read(cx).value().trim().to_owned();
        let hub = self.hub(cx);
        let cursor = if more { self.cursor.clone() } else { None };
        if more && (cursor.is_none() || self.searched.as_ref() != Some(&(hub, query.clone()))) {
            return;
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
                    library.partial = page.partial;
                    library.searched = Some(searched);
                }
                _ => library.failure(window, cx),
            },
        );
    }

    pub(super) fn install(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading || !self.available(cx) || self.target.busy(cx) {
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
            move |library, result, window, cx| match result {
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
            },
        );
    }

    pub(super) fn versions(
        &mut self,
        repository: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.loading || !self.available(cx) {
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
        if self.loading || !self.available(cx) {
            return;
        }
        let Some(repository) = self.repository.clone() else {
            return;
        };
        let Some(revision) = self.version.read(cx).selected_value().cloned() else {
            return;
        };
        self.definition = None;
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
                        && definition.validate().is_ok() =>
                {
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

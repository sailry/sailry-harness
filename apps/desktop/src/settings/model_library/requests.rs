use super::*;
use sailry_protocol::{Output, models::Search};

impl Library {
    pub(super) fn search(&mut self, more: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading || !self.available(cx) {
            return;
        }
        let query = self.query.read(cx).value().trim().to_owned();
        if query.is_empty() {
            return;
        }
        let hub = self.hub(cx);
        let cursor = if more { self.cursor.clone() } else { None };
        if more && (cursor.is_none() || self.searched.as_ref() != Some(&(hub, query.clone()))) {
            return;
        }
        self.loading = true;
        self.generation += 1;
        let generation = self.generation;
        let binding = self.binding.clone();
        let searched = (hub, query.clone());
        let kind = self.target.kind();
        let job = binding.runtime.spawn(async move {
            binding
                .client
                .execute(binding.client.prepare(Command::SearchModels(Search {
                    hub,
                    kind,
                    query,
                    cursor,
                })))
                .await
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |library, window, cx| {
                if library.generation != generation {
                    return;
                }
                library.loading = false;
                if !library.available(cx) {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Ok(Output::ModelSearch(page))) => {
                        if !more {
                            library.items.clear();
                            library.repository = None;
                            library.definition = None;
                        }
                        for item in page.items {
                            if !library
                                .items
                                .iter()
                                .any(|previous| previous.repository == item.repository)
                            {
                                library.items.push(item);
                            }
                        }
                        library.cursor = page.next_cursor;
                        library.partial = page.partial;
                        library.searched = Some(searched);
                    }
                    _ => library.failure(window, cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
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
        self.loading = true;
        self.repository = Some(repository.clone());
        self.definition = None;
        self.generation += 1;
        let generation = self.generation;
        let binding = self.binding.clone();
        let hub = self.hub(cx);
        let job = binding.runtime.spawn(async move {
            binding
                .client
                .execute(
                    binding
                        .client
                        .prepare(Command::ReadModelVersions { hub, repository }),
                )
                .await
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |library, window, cx| {
                if library.generation != generation {
                    return;
                }
                library.loading = false;
                if !library.available(cx) {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Ok(Output::ModelVersions(versions))) if !versions.is_empty() => {
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
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
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
        self.loading = true;
        self.definition = None;
        self.generation += 1;
        let generation = self.generation;
        let binding = self.binding.clone();
        let source = Source {
            hub: self.hub(cx),
            repository,
            revision,
        };
        let kind = self.target.kind();
        let expected = source.clone();
        let job = binding.runtime.spawn(async move {
            binding
                .client
                .execute(
                    binding
                        .client
                        .prepare(Command::ResolveModel { kind, source }),
                )
                .await
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |library, window, cx| {
                if library.generation != generation {
                    return;
                }
                library.loading = false;
                if !library.available(cx) {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Ok(Output::ModelDefinition(definition)))
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
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

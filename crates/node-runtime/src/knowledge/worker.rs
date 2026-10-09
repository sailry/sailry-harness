use super::*;
use index::{Document, Index};
use source::Entry;
use std::time::{Duration, Instant};

mod documents;
mod watches;

pub(super) fn run(
    owner: Weak<Service>,
    receiver: mpsc::Receiver<()>,
    mut index: Index,
    runtime: tokio::runtime::Handle,
) {
    sailry_embedding::background();
    let mut watches = watches::Watches::new(owner.clone());
    while wake(&receiver, watches.needs_poll()) {
        let Some(service) = owner.upgrade() else {
            break;
        };
        if service.stop.is_cancelled() {
            break;
        }
        let cancel = service
            .cancel
            .lock()
            .expect("knowledge cancellation lock is healthy")
            .clone();
        let result = pass(&service, &mut index, &runtime, &mut watches, &cancel);
        if result.is_err() {
            watches.finish(false);
        }
        if let Err(error) = result
            && !service.stop.is_cancelled()
            && error.code != ErrorCode::Cancelled
        {
            eprintln!("knowledge catalog is unavailable: {}", error.message);
        }
    }
}

fn wake(receiver: &mpsc::Receiver<()>, poll: bool) -> bool {
    let received = if poll {
        match receiver.recv_timeout(Duration::from_secs(5 * 60)) {
            Ok(()) => true,
            Err(mpsc::RecvTimeoutError::Timeout) => return true,
            Err(mpsc::RecvTimeoutError::Disconnected) => return false,
        }
    } else {
        receiver.recv().is_ok()
    };
    if !received {
        return false;
    }
    let started = Instant::now();
    loop {
        // Finish a burst after a quiet interval, with a bound for continuous edits.
        let remaining = Duration::from_secs(1).saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return true;
        }
        match receiver.recv_timeout(remaining.min(Duration::from_millis(250))) {
            Ok(()) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => return true,
            Err(mpsc::RecvTimeoutError::Disconnected) => return false,
        }
    }
}

fn pass(
    service: &Arc<Service>,
    index: &mut Index,
    runtime: &tokio::runtime::Handle,
    watches: &mut watches::Watches,
    cancel: &CancellationToken,
) -> Result<(), Fault> {
    watches.begin();
    let ingress = service.ingress.upgrade().ok_or_else(unavailable)?;
    let catalog = runtime.block_on(ingress.knowledge_catalog())?;
    service
        .models
        .configure(catalog.settings.clone(), &catalog.models)?;
    let installed = runtime
        .block_on(service.models.infos())?
        .into_iter()
        .filter(|info| info.state == ModelState::Installed)
        .map(|info| info.model)
        .collect::<BTreeSet<_>>();
    let current = catalog
        .collections
        .iter()
        .map(|stored| stored.collection.id)
        .collect::<Vec<_>>();
    index.retain(&current)?;
    let mut complete = true;
    let batch = Batch {
        service,
        ingress: &ingress,
        settings: &catalog.settings,
        catalog: &catalog,
        installed: &installed,
        cancel,
        runtime,
    };
    for stored in &catalog.collections {
        cancelled(cancel)?;
        if let Err(error) = collection(&batch, index, stored, watches) {
            cancelled(cancel)?;
            complete = false;
            eprintln!(
                "knowledge indexing paused for {}: {}",
                stored.collection.id, error.message
            );
            let (sources, chunks) = index.counts(binding(stored, &catalog)).unwrap_or_default();
            let progress = Progress {
                status: Status::Paused,
                sources,
                completed: sources,
                chunks,
                skipped: stored.collection.progress.skipped,
                model: None,
                generation: stored.collection.progress.generation,
            };
            let _ = runtime.block_on(ingress.knowledge_progress(
                stored.collection.id,
                progress.generation,
                &progress,
            ));
        }
    }
    cancelled(cancel)?;
    watches.finish(complete);
    Ok(())
}

struct Batch<'a> {
    service: &'a Service,
    ingress: &'a Arc<Ingress>,
    settings: &'a Settings,
    catalog: &'a super::Catalog,
    installed: &'a BTreeSet<Model>,
    cancel: &'a CancellationToken,
    runtime: &'a tokio::runtime::Handle,
}

fn collection(
    batch: &Batch<'_>,
    index: &mut Index,
    stored: &Stored,
    watches: &mut watches::Watches,
) -> Result<(), Fault> {
    let Batch {
        service,
        ingress,
        settings,
        catalog,
        installed,
        cancel,
        runtime,
    } = *batch;
    let binding = binding(stored, catalog);
    index.bind(binding)?;
    if stored.collection.paused {
        let (sources, chunks) = index.counts(binding)?;
        let progress = Progress {
            status: Status::Paused,
            sources,
            completed: sources,
            chunks,
            skipped: stored.collection.progress.skipped,
            model: None,
            generation: binding.generation,
        };
        runtime.block_on(ingress.knowledge_progress(binding.id, binding.generation, &progress))?;
        return Ok(());
    }
    let available = binding
        .model
        .map(|(model, _)| model)
        .filter(|model| installed.contains(model));
    let encoding = documents::Encoding {
        service,
        revision: settings.revision,
        model: available,
        cancel,
        runtime,
    };
    let mut progress = Progress {
        status: Status::Indexing,
        sources: 0,
        completed: 0,
        chunks: 0,
        skipped: 0,
        model: available,
        generation: binding.generation,
    };
    let mut active = false;
    let filters = stored
        .collection
        .definition
        .globs
        .iter()
        .map(|pattern| {
            globset::Glob::new(pattern)
                .map(|glob| glob.compile_matcher())
                .map_err(|_| Fault::invalid("invalid knowledge glob"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut locators = Vec::new();
    match &stored.collection.definition.source {
        Source::Storage { .. } => {
            let mut after = None;
            loop {
                cancelled(cancel)?;
                let page =
                    runtime.block_on(ingress.knowledge_sources(stored, after, 16, cancel))?;
                for record in page.records {
                    let document = Document {
                        locator: record.key.clone(),
                        revision: blake3::hash(
                            &serde_json::to_vec(&(record.hash, record.source_hash))
                                .map_err(|error| Fault::invalid(error.to_string()))?,
                        )
                        .to_hex()
                        .to_string(),
                        modified_ms: 0,
                        embedded: available.is_some(),
                        tags: stored
                            .collection
                            .definition
                            .tags
                            .iter()
                            .chain(&record.tags)
                            .cloned()
                            .collect(),
                    };
                    progress.sources += 1;
                    locators.push(document.locator.clone());
                    if !index.unchanged(binding, &document)? {
                        starting(batch, binding, &progress, &mut active)?;
                        let parts = encoding.parts(
                            &record.title,
                            &record.body,
                            &Position::Value {
                                key: record.source.key.clone(),
                                pointer: record.source.pointer.clone(),
                            },
                        )?;
                        cancelled(cancel)?;
                        if !runtime
                            .block_on(ingress.knowledge_source_current(stored, &record, cancel))?
                        {
                            return Err(Fault::new(
                                ErrorCode::RevisionConflict,
                                "knowledge storage source changed",
                            ));
                        }
                        index.replace(binding, &document, &parts)?;
                    }
                    progress.completed += 1;
                }
                after = page.after;
                if after.is_none() {
                    break;
                }
            }
        }
        source => {
            let root = if let Source::Worktree { worktree, .. } = source {
                Some(runtime.block_on(ingress.worktree_root(*worktree))?)
            } else {
                None
            };
            let watch = match source {
                Source::Worktree { .. } => root.clone(),
                Source::Directory { path } => Some(source::native(path)?),
                Source::File { path } => source::native(path)?.parent().map(Path::to_owned),
                Source::Storage { .. } => None,
            }
            .ok_or_else(unavailable)?;
            let mut scan = documents::scan(source, root.as_deref(), cancel.clone())?
                .with_ocr(ingress.ocr.clone());
            watches.add(&watch, scan.take_watches());
            let matches = |path: &str| {
                filters.is_empty()
                    || filters
                        .iter()
                        .any(|filter| filter.is_match(documents::relative(source, path)))
            };
            let mut last_progress = Instant::now();
            loop {
                let entry = scan.next_watched(
                    &mut |path, stat| {
                        if !matches(path) {
                            return Ok(true);
                        }
                        let cached = index.cached(
                            binding,
                            path,
                            stat,
                            &stored.collection.definition.tags,
                            available.is_some(),
                        )?;
                        if !cached {
                            starting(batch, binding, &progress, &mut active)?;
                        }
                        Ok(cached)
                    },
                    &mut |found| watches.add(&watch, found),
                );
                let Some(entry) = entry else {
                    break;
                };
                cancelled(cancel)?;
                match entry? {
                    Entry::Cached { path } => {
                        if !matches(&path) {
                            continue;
                        }
                        progress.sources += 1;
                        progress.completed += 1;
                        locators.push(path);
                    }
                    Entry::Skipped { excluded: true, .. } => {}
                    Entry::Skipped { path, reason, .. } => {
                        progress.skipped += 1;
                        eprintln!(
                            "knowledge source skipped for {} ({path}): {reason}",
                            binding.id
                        );
                    }
                    Entry::Document(document) => {
                        if !matches(&document.path) {
                            continue;
                        }
                        progress.sources += 1;
                        locators.push(document.path.clone());
                        let indexed = Document {
                            locator: document.path.clone(),
                            revision: document.revision.clone(),
                            modified_ms: document.modified_ms,
                            embedded: available.is_some(),
                            tags: stored.collection.definition.tags.clone(),
                        };
                        if !index.unchanged(binding, &indexed)? {
                            starting(batch, binding, &progress, &mut active)?;
                            let mut parts = Vec::new();
                            for part in &document.parts {
                                parts.extend(encoding.parts(
                                    &part.title,
                                    &part.text,
                                    &part.position,
                                )?);
                            }
                            cancelled(cancel)?;
                            if !documents::unchanged(
                                source,
                                root.as_deref(),
                                &document,
                                cancel.clone(),
                                ingress.ocr.clone(),
                            )? {
                                return Err(Fault::new(
                                    ErrorCode::RevisionConflict,
                                    "knowledge file changed before publication",
                                ));
                            }
                            if let Source::Worktree { worktree, .. } = source
                                && Some(runtime.block_on(ingress.worktree_root(*worktree))?) != root
                            {
                                return Err(Fault::new(
                                    ErrorCode::RevisionConflict,
                                    "knowledge worktree moved",
                                ));
                            }
                            index.replace(binding, &indexed, &parts)?;
                        }
                        index.remember(binding, &document.path, document.stat)?;
                        progress.completed += 1;
                    }
                }
                if active && last_progress.elapsed() >= Duration::from_millis(500) {
                    progress.chunks = index.counts(binding)?.1;
                    runtime.block_on(ingress.knowledge_progress(
                        binding.id,
                        binding.generation,
                        &progress,
                    ))?;
                    last_progress = Instant::now();
                }
            }
            if !scan.complete() {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "knowledge source scan was incomplete",
                ));
            }
        }
    }
    cancelled(cancel)?;
    index.prune(binding, &locators)?;
    (progress.sources, progress.chunks) = index.counts(binding)?;
    progress.completed = progress.sources;
    progress.status = Status::Ready;
    runtime.block_on(ingress.knowledge_progress(binding.id, binding.generation, &progress))?;
    Ok(())
}

fn starting(
    batch: &Batch<'_>,
    binding: index::Binding,
    progress: &Progress,
    active: &mut bool,
) -> Result<(), Fault> {
    if !*active {
        batch.runtime.block_on(batch.ingress.knowledge_progress(
            binding.id,
            binding.generation,
            progress,
        ))?;
        *active = true;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

fn cancelled(cancel: &CancellationToken) -> Result<(), Fault> {
    if cancel.is_cancelled() {
        Err(Fault::new(
            ErrorCode::Cancelled,
            "knowledge indexing was cancelled",
        ))
    } else {
        Ok(())
    }
}

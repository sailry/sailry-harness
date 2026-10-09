use super::*;
use index::{Document, Index};
use source::Entry;
use std::time::Duration;

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
    while let Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) =
        receiver.recv_timeout(Duration::from_secs(30))
    {
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
        if let Err(error) = result
            && !service.stop.is_cancelled()
        {
            eprintln!("knowledge catalog is unavailable: {}", error.message);
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
    let mut roots = BTreeSet::new();
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
        if let Err(error) = collection(&batch, index, stored, &mut roots) {
            cancelled(cancel)?;
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
    watches.replace(roots);
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
    roots: &mut BTreeSet<PathBuf>,
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
    runtime.block_on(ingress.knowledge_progress(binding.id, binding.generation, &progress))?;
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
                Source::Directory { path } => Some(PathBuf::from(path)),
                Source::File { path } => Path::new(path).parent().map(Path::to_owned),
                Source::Storage { .. } => None,
            };
            if let Some(watch) = watch {
                roots.insert(watch);
            }
            let scan = documents::scan(source, root.as_deref(), cancel.clone())?;
            let mut last_progress = std::time::Instant::now();
            for entry in scan {
                cancelled(cancel)?;
                match entry? {
                    Entry::Skipped { path, reason } => {
                        progress.skipped += 1;
                        eprintln!(
                            "knowledge source skipped for {} ({path}): {reason}",
                            binding.id
                        );
                    }
                    Entry::Document(document) => {
                        let relative = documents::relative(source, &document.path);
                        if !filters.is_empty()
                            && !filters.iter().any(|filter| filter.is_match(relative))
                        {
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
                        progress.completed += 1;
                    }
                }
                if last_progress.elapsed() >= Duration::from_millis(500) {
                    progress.chunks = index.counts(binding)?.1;
                    runtime.block_on(ingress.knowledge_progress(
                        binding.id,
                        binding.generation,
                        &progress,
                    ))?;
                    last_progress = std::time::Instant::now();
                }
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

//! Citations resolve indexed identities through current grants and source capabilities.
use super::*;
use sailry_protocol::WorktreeId;

#[cfg(test)]
mod tests;

impl Service {
    pub(crate) async fn citation(
        &self,
        request: &Request,
        input: &CitationRequest,
        cancel: &CancellationToken,
    ) -> Result<CitationPage, Fault> {
        input.validate()?;
        check_cancel(cancel)?;
        let ingress = self.ingress.upgrade().ok_or_else(unavailable)?;
        let stored = permitted(&ingress, request, input.collection).await?;
        let catalog = ingress.knowledge_catalog().await?;
        let captured = binding(&stored, &catalog);
        let path = self.path.clone();
        let selected = input.clone();
        let indexed = tokio::task::spawn_blocking(move || {
            index::Index::reader(&path)?.citation(captured, &selected)
        })
        .await
        .map_err(|_| unavailable())?;
        let indexed = match indexed {
            Ok(record) => record,
            Err(error)
                if matches!(
                    error.code,
                    ErrorCode::RevisionConflict | ErrorCode::NotFound
                ) =>
            {
                self.refresh();
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        check_cancel(cancel)?;

        let (text, worktree) = match &stored.collection.definition.source {
            Source::Storage { .. } => {
                let record = ingress
                    .knowledge_source(&stored, &indexed.citation.path, cancel)
                    .await?
                    .ok_or_else(changed)?;
                let revision = blake3::hash(
                    &serde_json::to_vec(&(record.hash, record.source_hash))
                        .map_err(|error| Fault::invalid(error.to_string()))?,
                )
                .to_hex()
                .to_string();
                let position = Position::Value {
                    key: record.source.key.clone(),
                    pointer: record.source.pointer.clone(),
                };
                if revision != indexed.citation.revision
                    || position != indexed.citation.position
                    || !ingress
                        .knowledge_source_current(&stored, &record, cancel)
                        .await?
                {
                    self.refresh();
                    return Err(changed());
                }
                (record.body, None)
            }
            source => {
                let root = match source {
                    Source::Worktree { worktree, .. } => {
                        Some(ingress.worktree_root(*worktree).await?)
                    }
                    _ => None,
                };
                let definition = stored.collection.definition.clone();
                let citation = indexed.citation.clone();
                let captured_root = root.clone();
                let stop = cancel.clone();
                let complete = input.context;
                let ocr = ingress.ocr.clone();
                let text = tokio::task::spawn_blocking(move || {
                    context_with(
                        &definition,
                        captured_root.as_deref(),
                        &citation,
                        stop,
                        complete,
                        Some(ocr),
                    )
                })
                .await
                .map_err(|_| unavailable())?;
                let text = match text {
                    Ok(text) => text,
                    Err(error) if error.code == ErrorCode::RevisionConflict => {
                        self.refresh();
                        return Err(error);
                    }
                    Err(error) => return Err(error),
                };
                let worktree = match source {
                    Source::Worktree { worktree, .. } => {
                        if Some(ingress.worktree_root(*worktree).await?) != root {
                            return Err(changed());
                        }
                        Some(*worktree)
                    }
                    _ => None,
                };
                (text, worktree)
            }
        };
        check_cancel(cancel)?;
        let current = permitted(&ingress, request, input.collection).await?;
        if current.collection.id != stored.collection.id
            || current.collection.revision != stored.collection.revision
            || current.collection.progress.generation != stored.collection.progress.generation
            || current.collection.rebuild != stored.collection.rebuild
            || current.collection.paused != stored.collection.paused
            || current.collection.owner != stored.collection.owner
            || current.collection.definition != stored.collection.definition
            || current.automatic != stored.automatic
            || current.capture.package != stored.capture.package
            || current.capture.worktree != stored.capture.worktree
            || current.capture.session != stored.capture.session
            || ingress.knowledge_catalog().await?.settings.revision != catalog.settings.revision
        {
            return Err(changed());
        }
        page(indexed, &text, input, worktree)
    }
}

async fn permitted(
    ingress: &Ingress,
    request: &Request,
    collection: sailry_protocol::KnowledgeId,
) -> Result<Stored, Fault> {
    ingress
        .knowledge_allowed(request)
        .await?
        .into_iter()
        .find(|stored| stored.collection.id == collection)
        .ok_or_else(|| {
            Fault::new(
                ErrorCode::PermissionDenied,
                "knowledge collection is not accessible",
            )
        })
}

fn check_cancel(cancel: &CancellationToken) -> Result<(), Fault> {
    if cancel.is_cancelled() {
        Err(Fault::new(
            ErrorCode::Cancelled,
            "knowledge citation read was cancelled",
        ))
    } else {
        Ok(())
    }
}

fn changed() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "knowledge source changed; refresh its results",
    )
}

#[cfg(test)]
fn context(
    definition: &Definition,
    root: Option<&Path>,
    citation: &Citation,
    cancel: CancellationToken,
    complete: bool,
) -> Result<String, Fault> {
    context_with(definition, root, citation, cancel, complete, None)
}

fn context_with(
    definition: &Definition,
    root: Option<&Path>,
    citation: &Citation,
    cancel: CancellationToken,
    complete: bool,
    ocr: Option<std::sync::Arc<crate::ocr::Service>>,
) -> Result<String, Fault> {
    let locator = Path::new(&citation.path);
    let (mut scan, relative) = match &definition.source {
        Source::Worktree { path, .. } => {
            let base = if path == "." {
                Path::new("")
            } else {
                Path::new(path)
            };
            if !locator.starts_with(base) {
                return Err(changed());
            }
            (
                source::worktree(root.ok_or_else(unavailable)?, &citation.path, cancel)
                    .map_err(source_error)?,
                locator,
            )
        }
        Source::Directory { path } => {
            let relative = locator.strip_prefix(path).map_err(|_| changed())?;
            if relative.as_os_str().is_empty() {
                return Err(changed());
            }
            (
                source::within(path, relative.to_str().ok_or_else(changed)?, cancel)
                    .map_err(source_error)?,
                relative,
            )
        }
        Source::File { path } => {
            if path != &citation.path {
                return Err(changed());
            }
            let relative = locator.file_name().map(Path::new).ok_or_else(changed)?;
            (
                source::controller(&definition.source, cancel).map_err(source_error)?,
                relative,
            )
        }
        Source::Storage { .. } => return Err(Fault::invalid("file citation expected")),
    };
    if !definition.globs.is_empty()
        && !definition
            .globs
            .iter()
            .map(|glob| {
                globset::Glob::new(glob)
                    .map(|glob| glob.compile_matcher().is_match(relative))
                    .map_err(|error| Fault::invalid(error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .any(|matched| matched)
    {
        return Err(changed());
    }
    if let Some(ocr) = ocr {
        scan = scan.with_ocr(ocr);
    }
    if let Some(entry) = scan.next() {
        match entry? {
            source::Entry::Document(document) => {
                if document.path != citation.path || document.revision != citation.revision {
                    return Err(changed());
                }
                return if complete {
                    Ok(document
                        .parts
                        .iter()
                        .map(|part| part.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n\n"))
                } else {
                    position(&document.parts, &citation.position)
                };
            }
            source::Entry::Cached { .. } | source::Entry::Skipped { .. } => return Err(changed()),
        }
    }
    Err(changed())
}

fn source_error(error: Fault) -> Fault {
    if error.code == ErrorCode::NotFound {
        changed()
    } else {
        error
    }
}

fn position(parts: &[source::Part], selected: &Position) -> Result<String, Fault> {
    if let Position::Lines { start, end } = selected {
        if *start == 0 || end < start {
            return Err(changed());
        }
        let part = parts
            .iter()
            .find(|part| matches!(part.position, Position::Lines { .. }))
            .ok_or_else(changed)?;
        let lines = part.text.lines().collect::<Vec<_>>();
        if *end as usize > lines.len() {
            return Err(changed());
        }
        return Ok(lines[*start as usize - 1..*end as usize].join("\n"));
    }
    let selected = parts
        .iter()
        .filter(|part| &part.position == selected)
        .map(|part| part.text.as_str())
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err(changed());
    }
    Ok(selected.join("\n\n"))
}

fn page(
    record: index::Record,
    text: &str,
    input: &CitationRequest,
    worktree: Option<WorktreeId>,
) -> Result<CitationPage, Fault> {
    let start = input.offset as usize;
    if start > text.len() || !text.is_char_boundary(start) {
        return Err(Fault::invalid(
            "knowledge citation offset is not a text boundary",
        ));
    }
    let mut end = start
        .saturating_add(input.limit.min(64 * 1024) as usize)
        .min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    if start < text.len() && start == end {
        return Err(Fault::invalid(
            "knowledge citation limit cannot fit the next character",
        ));
    }
    Ok(CitationPage {
        citation: record.citation,
        title: record.title,
        text: text[start..end].into(),
        offset: input.offset,
        next_offset: (end < text.len()).then_some(end as u32),
        worktree,
        context: input.context,
        untrusted: true,
    })
}

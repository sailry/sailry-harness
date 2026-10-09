use super::super::*;
use source::{Entry, Scan, SourceDoc};

pub(super) fn scan(
    source: &Source,
    root: Option<&Path>,
    cancel: CancellationToken,
) -> Result<Scan, Fault> {
    match source {
        Source::Worktree { path, .. } => {
            source::worktree(root.ok_or_else(unavailable)?, path, cancel)
        }
        Source::Directory { .. } | Source::File { .. } => source::controller(source, cancel),
        Source::Storage { .. } => Err(Fault::invalid("file source expected")),
    }
}

pub(super) fn relative<'a>(source: &Source, path: &'a str) -> &'a Path {
    match source {
        Source::Directory { path: base } => Path::new(path)
            .strip_prefix(base)
            .unwrap_or(Path::new(path)),
        Source::File { .. } => Path::new(path)
            .file_name()
            .map_or(Path::new(path), Path::new),
        _ => Path::new(path),
    }
}

pub(super) fn unchanged(
    source: &Source,
    root: Option<&Path>,
    document: &SourceDoc,
    cancel: CancellationToken,
) -> Result<bool, Fault> {
    let selected = match source {
        Source::Worktree { worktree, .. } => Source::Worktree {
            worktree: *worktree,
            path: document.path.clone(),
        },
        _ => Source::File {
            path: document.path.clone(),
        },
    };
    for entry in scan(&selected, root, cancel)? {
        if let Entry::Document(current) = entry? {
            return Ok(current.path == document.path
                && current.revision == document.revision
                && current.modified_ms == document.modified_ms);
        }
    }
    Ok(false)
}

pub(super) struct Encoding<'a> {
    pub service: &'a Service,
    pub revision: u64,
    pub model: Option<Model>,
    pub cancel: &'a CancellationToken,
    pub runtime: &'a tokio::runtime::Handle,
}

impl Encoding<'_> {
    pub(super) fn parts(
        &self,
        title: &str,
        text: &str,
        position: &Position,
    ) -> Result<Vec<index::Part>, Fault> {
        let offsets = if let Some(model) = self.model {
            self.runtime.block_on(self.service.models.offsets(
                model,
                self.revision,
                text.into(),
                self.cancel,
            ))?
        } else {
            Vec::new()
        };
        let offsets = if offsets.is_empty() {
            text.char_indices()
                .map(|(start, character)| (start, start + character.len_utf8()))
                .collect()
        } else {
            offsets
        };
        let chunks = chunks::split(text, &offsets)?;
        let mut parts = chunks
            .iter()
            .map(|chunk| {
                let position = match position {
                    Position::Lines { start, .. } => Position::Lines {
                        start: start.saturating_add(chunk.first_line - 1),
                        end: start.saturating_add(chunk.last_line - 1),
                    },
                    position => position.clone(),
                };
                index::Part {
                    title: chunk.heading.clone().unwrap_or_else(|| title.into()),
                    text: text[chunk.start..chunk.end].into(),
                    position,
                    vector: None,
                }
            })
            .collect::<Vec<_>>();
        if let Some(model) = self.model {
            for batch in parts.chunks_mut(4) {
                super::cancelled(self.cancel)?;
                let vectors = self.runtime.block_on(self.service.models.encode(
                    model,
                    self.revision,
                    batch.iter().map(|part| part.text.clone()).collect(),
                    models::Lane::Index,
                    self.cancel,
                ))?;
                if vectors.len() != batch.len() {
                    return Err(Fault::new(
                        ErrorCode::Internal,
                        "embedding batch count differs",
                    ));
                }
                for (part, vector) in batch.iter_mut().zip(vectors) {
                    // An all-unknown static-model input has no semantic direction; keep its keyword text.
                    if vector.iter().any(|value| *value != 0.) {
                        part.vector = Some(vector::Vector::quantize(&vector)?)
                    }
                }
            }
        }
        Ok(parts)
    }
}

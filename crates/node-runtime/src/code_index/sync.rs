//! Scan, bounded parallel reads/parses, then short writer transactions.
//! The staged approach follows ast-index v3.56.0 (MIT); see NOTICE.
use super::*;
use index::Cached;
use parser::Parsed;
use std::collections::VecDeque;

#[derive(Clone, Default)]
pub(super) struct Report {
    pub visited: usize,
    pub skipped: usize,
    pub partial: bool,
    pub truncated: bool,
    pub issues: Vec<Issue>,
}

impl Report {
    fn record(&mut self, path: &str, state: &index::State) {
        self.partial |= state.partial;
        self.truncated |= state.truncated;
        if state.status != Status::Parsed {
            self.skipped += 1;
            if self.issues.len() < 100 {
                self.issues.push(Issue {
                    path: path.into(),
                    status: state.status,
                });
            }
        }
    }
}

pub(super) struct Limits {
    pub files: usize,
    pub batches: usize,
    pub deadline: Instant,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            files: MAX_FILES,
            batches: usize::MAX,
            deadline: Instant::now() + Duration::from_secs(15),
        }
    }
}

#[derive(Default)]
pub(super) struct Progress {
    scan: Option<Scan>,
}

impl Progress {
    #[cfg(test)]
    pub(super) fn remaining(&self) -> Option<usize> {
        self.scan.as_ref().map(|scan| scan.pending.len())
    }

    #[cfg(test)]
    pub(super) fn next(&self) -> Option<&str> {
        self.scan.as_ref()?.pending.front().map(String::as_str)
    }
}

struct Scan {
    generation: u64,
    limit: usize,
    pending: VecDeque<String>,
    versions: BTreeMap<String, u64>,
    seen: BTreeSet<String>,
    complete: bool,
    report: Report,
}

impl Scan {
    fn changed(&mut self, paths: &BTreeMap<String, u64>) {
        for (path, generation) in paths.iter().rev() {
            if self.versions.get(path) == Some(generation)
                || !self.seen.contains(path)
                || !parser::candidate(path)
            {
                continue;
            }
            // New paths must pass the next ignore-aware walk before entering this cursor.
            self.pending.retain(|pending| pending != path);
            self.pending.push_front(path.clone());
            self.versions.insert(path.clone(), *generation);
        }
    }
}

impl Service {
    pub(super) async fn read(
        &self,
        worktree: &Worktree,
        path: String,
        cached: Option<&Cached>,
        resources: Arc<resources::Resources>,
        stop: &CancellationToken,
        deadline: Instant,
    ) -> Result<Option<Parsed>, Fault> {
        let permit = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(cancelled()),
            _ = tokio::time::sleep_until(deadline.into()) => return Err(Fault::new(ErrorCode::Busy,"code indexing budget exhausted")),
            permit = self.parsing.clone().acquire_owned() => permit.map_err(error)?,
        };
        let root = worktree.root.clone();
        let stop = stop.clone();
        let node_stop = worktree.stop.clone();
        let cached = cached.map(|cached| {
            (
                cached.hash.clone(),
                cached.fingerprint.clone(),
                cached.state.status,
            )
        });
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            check(&stop)?;
            check(&node_stop)?;
            let control = crate::files::Control::until(stop.clone(), deadline);
            let source = crate::files::index_text(root.as_path(), &path, MAX_FILE_BYTES, &control)?;
            let Some(source) = source else {
                // A fresh size check proves the same omitted state. There are no
                // symbols or source revision to replace while the file is oversized.
                if cached.as_ref().is_some_and(|(_, rules, status)| {
                    rules == &resources.fingerprint && *status == Status::TooLarge
                }) {
                    return Ok(None);
                }
                return Ok(Some(Parsed::omitted(path, Status::TooLarge)));
            };
            let hash = blake3::hash(source.as_bytes()).to_hex().to_string();
            if cached.is_some_and(|(previous, rules, _)| {
                previous.as_deref() == Some(hash.as_str()) && rules == resources.fingerprint
            }) {
                return Ok(None);
            }
            resources.parser.parse(path, &source, &stop).map(Some)
        })
        .await
        .map_err(error)?
    }

    pub(super) async fn synchronize(
        &self,
        worktree: &Arc<Worktree>,
        index: &index::Index,
        resources: Arc<resources::Resources>,
        stop: &CancellationToken,
        progress: &mut Progress,
        limits: Limits,
    ) -> Result<Report, Fault> {
        check(stop)?;
        let (generation, dirty_paths) = {
            let dirty = worktree.dirty.lock().unwrap();
            (dirty.generation, dirty.paths.clone())
        };
        if progress
            .scan
            .as_ref()
            .is_none_or(|scan| scan.limit != limits.files)
        {
            let root = worktree.root.clone();
            let scan_stop = stop.clone();
            let deadline = limits.deadline;
            let limit = limits.files;
            let listing = tokio::task::spawn_blocking(move || {
                let control = crate::files::Control::until(scan_stop, deadline);
                crate::files::walk::collect_bounded(root.as_path(), "", limit, &control)
            })
            .await
            .map_err(error)?;
            check(stop)?;
            let listing = match listing {
                Ok(listing) => listing,
                Err(fault) if fault.code == ErrorCode::Busy => {
                    return Ok(Report {
                        partial: true,
                        truncated: true,
                        ..Default::default()
                    });
                }
                Err(fault) => return Err(fault),
            };
            let seen = listing
                .entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect();
            let mut pending: Vec<_> = listing
                .entries
                .into_iter()
                .filter(|entry| parser::candidate(&entry.path))
                .map(|entry| entry.path)
                .collect();
            pending.sort_by(|left, right| {
                (!dirty_paths.contains_key(left), left)
                    .cmp(&(!dirty_paths.contains_key(right), right))
            });
            progress.scan = Some(Scan {
                generation,
                limit,
                pending: pending.into(),
                versions: dirty_paths,
                seen,
                complete: !listing.report.partial && !listing.report.truncated,
                report: Report {
                    visited: listing.report.visited,
                    skipped: listing.report.skipped,
                    partial: listing.report.partial,
                    truncated: listing.report.truncated,
                    issues: Vec::new(),
                },
            });
        }
        let scan = progress.scan.as_mut().expect("scan was prepared");
        let mut files = index.files().await?;
        let mut batches = 0;
        while !scan.pending.is_empty()
            && batches < limits.batches
            && Instant::now() < limits.deadline
        {
            check(stop)?;
            check(&worktree.stop)?;
            // Root invalidations can come from ignored build output. Finish this
            // cursor before reconciling them; exact writes still move to its front.
            scan.changed(&worktree.dirty.lock().unwrap().paths);
            let batch: Vec<_> = scan.pending.iter().take(BATCH).cloned().collect();
            // Shared CPU permits bound all worktrees; no writer is held while parsing.
            let parsed = futures::future::join_all(batch.iter().map(|path| async {
                self.read(
                    worktree,
                    path.clone(),
                    files.get(path),
                    resources.clone(),
                    stop,
                    limits.deadline,
                )
                .await
            }))
            .await;
            let mut changes = Vec::new();
            let mut states = Vec::new();
            let mut exhausted = false;
            for (path, result) in batch.iter().zip(parsed) {
                match result {
                    Ok(Some(file)) => changes.push(file),
                    Ok(None) => {
                        if let Some(cached) = files.get(path) {
                            states.push((path.clone(), cached.state.clone()));
                        }
                    }
                    Err(fault)
                        if matches!(
                            fault.code,
                            ErrorCode::Cancelled | ErrorCode::NotConfigured
                        ) =>
                    {
                        return Err(fault);
                    }
                    Err(fault) if fault.code == ErrorCode::Busy => exhausted = true,
                    Err(_) => changes.push(Parsed::omitted(path.clone(), Status::Invalid)),
                }
            }
            if exhausted {
                break;
            }
            #[cfg(test)]
            {
                let pause = worktree.prepared.lock().unwrap().take();
                if let Some((entered, resume)) = pause {
                    let _ = entered.send(());
                    let _ = resume.await;
                }
            }
            let committed = commit(
                index,
                &changes,
                &resources.fingerprint,
                stop,
                limits.deadline,
            )
            .await?;
            let mut completed = BTreeSet::new();
            for file in &changes[..committed] {
                let state = index::State::from(file);
                scan.report.record(&file.path, &state);
                files.insert(
                    file.path.clone(),
                    Cached {
                        hash: file.hash.clone(),
                        fingerprint: resources.fingerprint.clone(),
                        state,
                    },
                );
                completed.insert(file.path.as_str());
            }
            for (path, state) in &states {
                scan.report.record(path, state);
                completed.insert(path.as_str());
            }
            // Keep every committed file and cached result when a later transaction
            // reaches the work budget; the remaining atomic files stay queued.
            scan.pending
                .retain(|path| !completed.contains(path.as_str()));
            batches += 1;
            if committed < changes.len() {
                break;
            }
        }
        check(stop)?;
        let current = worktree.dirty.lock().unwrap().generation == scan.generation;
        let mut finished = scan.pending.is_empty();
        if finished && scan.complete && current {
            // Absence is proven only by a complete walk. Committed pruning batches
            // also survive a budget boundary; unfinished deletions remain pending.
            finished = match tokio::time::timeout_at(
                limits.deadline.into(),
                index.prune(&scan.seen, stop),
            )
            .await
            {
                Ok(result) => {
                    result?;
                    true
                }
                Err(_) => false,
            };
        }
        let mut report = scan.report.clone();
        let mut dirty = worktree.dirty.lock().unwrap();
        report.partial |= dirty.watch_failed || !current || !finished;
        report.truncated |= !finished || !current;
        if finished && scan.complete {
            report.partial |= !dirty.finish(scan.generation);
        }
        if finished {
            progress.scan = None;
        }
        Ok(report)
    }
}

async fn commit(
    index: &index::Index,
    files: &[Parsed],
    fingerprint: &str,
    stop: &CancellationToken,
    deadline: Instant,
) -> Result<usize, Fault> {
    let mut start = 0;
    while start < files.len() {
        check(stop)?;
        let mut end = start + 1;
        let mut symbols = files[start].symbols.len();
        while end < files.len() && symbols + files[end].symbols.len() <= index::BATCH_SYMBOLS {
            symbols += files[end].symbols.len();
            end += 1;
        }
        if Instant::now() >= deadline {
            break;
        }
        // The work budget stops new transactions, not an in-flight atomic file.
        // Caller cancellation and the admitted query deadline still roll it back.
        index.replace(&files[start..end], fingerprint, stop).await?;
        start = end;
    }
    Ok(start)
}

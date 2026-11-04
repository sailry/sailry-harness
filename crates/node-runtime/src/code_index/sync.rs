//! Scan, bounded parallel reads/parses, then short writer transactions.
//! The staged approach follows ast-index v3.56.0 (MIT); see NOTICE.
use super::*;
use index::Cached;
use parser::Parsed;
use std::collections::VecDeque;

/// Scan-wide reconcile state plus per-file problems that searches scope by path.
#[derive(Clone, Default)]
pub(super) struct Report {
    pub visited: usize,
    /// Walk entries omitted without a path, such as links and special files.
    pub skipped: usize,
    /// The reconcile itself was cut short or could not prove freshness.
    pub partial: bool,
    pub truncated: bool,
    problems: BTreeMap<String, Problem>,
}

#[derive(Clone)]
struct Problem {
    status: Status,
    partial: bool,
    truncated: bool,
}

/// Search metadata limited to one requested path prefix.
pub(super) struct Scoped {
    pub skipped: usize,
    pub partial: bool,
    pub truncated: bool,
    pub issues: Vec<Issue>,
}

impl Report {
    /// Another refresh or an exhausted walk cannot prove the index is current.
    pub(super) fn interrupted() -> Self {
        Self {
            partial: true,
            truncated: true,
            ..Default::default()
        }
    }

    fn record(&mut self, path: &str, state: &index::State) {
        // Files without a supported grammar are expected and never degrade a result.
        let problem = state.status != Status::Unsupported
            && (state.status != Status::Parsed || state.partial || state.truncated);
        if problem {
            self.problems.insert(
                path.into(),
                Problem {
                    status: state.status,
                    partial: state.partial,
                    truncated: state.truncated,
                },
            );
        } else {
            self.problems.remove(path);
        }
    }

    pub(super) fn scoped(&self, filter: Option<&str>) -> Scoped {
        let mut scoped = Scoped {
            skipped: self.skipped,
            partial: self.partial,
            truncated: self.truncated,
            issues: Vec::new(),
        };
        for (path, problem) in &self.problems {
            if filter.is_some_and(|filter| {
                path != filter
                    && !path
                        .strip_prefix(filter)
                        .is_some_and(|rest| rest.starts_with('/'))
            }) {
                continue;
            }
            scoped.partial |= problem.partial;
            scoped.truncated |= problem.truncated;
            if problem.status != Status::Parsed {
                scoped.skipped += 1;
                if scoped.issues.len() < MAX_ISSUES {
                    scoped.issues.push(Issue {
                        path: path.clone(),
                        status: problem.status,
                    });
                }
            }
        }
        scoped
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
    records: Records,
}

/// File records kept between reconciles. Every content check still reads and
/// hashes the file; this only avoids reloading the whole table per query.
#[derive(Default)]
struct Records {
    /// The index revision `entries` reflects; `None` once another writer may
    /// have committed in between.
    revision: Option<u64>,
    entries: BTreeMap<String, Cached>,
}

impl Records {
    async fn load(&mut self, index: &index::Index) -> Result<(), Fault> {
        let current = index.revision().await?;
        if self.revision != Some(current) {
            let (revision, entries) = index.snapshot().await?;
            self.revision = Some(revision);
            self.entries = entries;
        }
        Ok(())
    }

    /// Follows one transaction committed by the sync owner after its changes
    /// were applied to `entries`. A gap means another writer interleaved.
    fn advance(&mut self, revision: u64) {
        self.revision = self
            .revision
            .filter(|previous| previous.wrapping_add(1) == revision)
            .map(|_| revision);
    }

    fn insert(&mut self, file: &Parsed, fingerprint: &str) -> index::State {
        let state = index::State::from(file);
        self.entries.insert(
            file.path.clone(),
            Cached {
                hash: file.hash.clone(),
                fingerprint: fingerprint.to_owned(),
                state: state.clone(),
            },
        );
        state
    }
}

impl Progress {
    /// Reuses validated records for a single-file refresh under the sync lock.
    pub(super) async fn cached(
        &mut self,
        index: &index::Index,
        path: &str,
    ) -> Result<Option<&Cached>, Fault> {
        self.records.load(index).await?;
        Ok(self.records.entries.get(path))
    }

    pub(super) fn replaced(&mut self, file: &Parsed, fingerprint: &str, revision: u64) {
        self.records.insert(file, fingerprint);
        self.records.advance(revision);
    }

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

/// Indexed content hash, extraction rules and status of one file.
pub(super) type Prior = (Option<String>, String, Status);

pub(super) struct Refresh {
    pub changed: bool,
    pub checked: BTreeSet<String>,
}

pub(super) fn prior(cached: Option<&Cached>) -> Option<Prior> {
    cached.map(|cached| {
        (
            cached.hash.clone(),
            cached.fingerprint.clone(),
            cached.state.status,
        )
    })
}

impl Service {
    /// Reads, fully hashes and, when changed, parses files in order on one
    /// blocking worker that holds one shared CPU permit.
    pub(super) async fn read(
        &self,
        worktree: &Worktree,
        directory: Arc<cap_std::fs::Dir>,
        files: Vec<(String, Option<Prior>)>,
        resources: Arc<resources::Resources>,
        stop: &CancellationToken,
        deadline: Instant,
    ) -> Result<Vec<Result<Option<Parsed>, Fault>>, Fault> {
        let permit = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(cancelled()),
            _ = tokio::time::sleep_until(deadline.into()) => return Err(Fault::new(ErrorCode::Busy,"code indexing budget exhausted")),
            permit = self.parsing.clone().acquire_owned() => permit.map_err(error)?,
        };
        let stop = stop.clone();
        let node_stop = worktree.stop.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let control = crate::files::Control::until(stop.clone(), deadline);
            files
                .into_iter()
                .map(|(path, prior)| {
                    check(&stop)?;
                    check(&node_stop)?;
                    Ok(inspect(
                        &directory, path, prior, &resources, &control, &stop,
                    ))
                })
                .collect()
        })
        .await
        .map_err(error)?
    }

    /// Re-reads the files named by a result page under the sync lock and
    /// replaces changed ones, so returned revisions match disk even when a
    /// path-less watch event has not reached them in the scan cursor.
    pub(super) async fn refresh(
        &self,
        worktree: &Arc<Worktree>,
        index: &index::Index,
        progress: &mut Progress,
        paths: BTreeSet<String>,
        stop: &CancellationToken,
        deadline: Instant,
    ) -> Result<Refresh, Fault> {
        if paths.is_empty() {
            return Ok(Refresh {
                changed: false,
                checked: BTreeSet::new(),
            });
        }
        let resources = self.resources().await?;
        progress.records.load(index).await?;
        let files = paths
            .into_iter()
            .map(|path| {
                let prior = prior(progress.records.entries.get(&path));
                (path, prior)
            })
            .collect::<Vec<_>>();
        let names = files
            .iter()
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        let results = self
            .read(
                worktree,
                open(worktree).await?,
                files,
                resources.clone(),
                stop,
                deadline,
            )
            .await?;
        let mut changes = Vec::new();
        let mut checked = BTreeSet::new();
        for (path, result) in names.into_iter().zip(results) {
            match result {
                Ok(Some(file)) => {
                    checked.insert(path);
                    changes.push(file);
                }
                Ok(None) => {
                    checked.insert(path);
                }
                Err(fault)
                    if matches!(fault.code, ErrorCode::Cancelled | ErrorCode::NotConfigured) =>
                {
                    return Err(fault);
                }
                // The scan resumes files left by the refresh budget.
                Err(fault) if fault.code == ErrorCode::Busy => {}
                Err(_) => {
                    checked.insert(path.clone());
                    changes.push(Parsed::omitted(path, Status::Invalid));
                }
            }
        }
        let revision = index
            .replace(&changes, &resources.fingerprint, stop)
            .await?;
        if let Some(revision) = revision {
            for file in &changes {
                progress.records.insert(file, &resources.fingerprint);
            }
            progress.records.advance(revision);
        }
        Ok(Refresh {
            changed: revision.is_some(),
            checked,
        })
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
                Err(fault) if fault.code == ErrorCode::Busy => return Ok(Report::interrupted()),
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
                    problems: BTreeMap::new(),
                },
            });
        }
        let Progress {
            scan: slot,
            records,
        } = progress;
        let scan = slot.as_mut().expect("scan was prepared");
        records.load(index).await?;
        // One confined root handle serves this segment instead of reopening
        // every root ancestor per file; the next segment opens it again.
        let directory = if scan.pending.is_empty() {
            None
        } else {
            Some(open(worktree).await?)
        };
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
            // Each permit covers a short ordered run, not one blocking task per file.
            let runs = batch.chunks(BATCH.div_ceil(READERS));
            let parsed = futures::future::join_all(runs.clone().map(|run| {
                let files = run
                    .iter()
                    .map(|path| (path.clone(), prior(records.entries.get(path))))
                    .collect();
                self.read(
                    worktree,
                    directory.clone().expect("pending files have a root"),
                    files,
                    resources.clone(),
                    stop,
                    limits.deadline,
                )
            }))
            .await
            .into_iter()
            .zip(runs)
            .flat_map(|(result, run)| result.unwrap_or_else(|fault| vec![Err(fault); run.len()]));
            let mut changes = Vec::new();
            let mut states = Vec::new();
            let mut exhausted = false;
            for (path, result) in batch.iter().zip(parsed) {
                match result {
                    Ok(Some(file)) => changes.push(file),
                    Ok(None) => {
                        if let Some(cached) = records.entries.get(path) {
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
            let (committed, revisions) = commit(
                index,
                &changes,
                &resources.fingerprint,
                stop,
                limits.deadline,
            )
            .await?;
            let mut completed = BTreeSet::new();
            for file in &changes[..committed] {
                let state = records.insert(file, &resources.fingerprint);
                scan.report.record(&file.path, &state);
                completed.insert(file.path.as_str());
            }
            for revision in revisions {
                records.advance(revision);
            }
            for (path, state) in &states {
                scan.report.record(path, state);
                completed.insert(path.as_str());
            }
            // Keep every committed file and cached result when a later transaction
            // reaches the work budget; the remaining atomic files stay queued.
            // The batch is still the cursor front, so only that front is filtered.
            let kept: Vec<_> = scan
                .pending
                .drain(..batch.len())
                .filter(|path| !completed.contains(path.as_str()))
                .collect();
            for path in kept.into_iter().rev() {
                scan.pending.push_front(path);
            }
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
                prune(index, records, &scan.seen, stop),
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
            *slot = None;
        }
        Ok(report)
    }
}

pub(super) async fn open(worktree: &Worktree) -> Result<Arc<cap_std::fs::Dir>, Fault> {
    let root = worktree.root.clone();
    tokio::task::spawn_blocking(move || crate::files::path::root(&root).map(Arc::new))
        .await
        .map_err(error)?
}

/// Removes records absent from a complete walk in committed batches of 64.
async fn prune(
    index: &index::Index,
    records: &mut Records,
    seen: &BTreeSet<String>,
    stop: &CancellationToken,
) -> Result<(), Fault> {
    // Revalidate so records committed by any other writer are also pruned.
    records.load(index).await?;
    let absent: Vec<_> = records
        .entries
        .keys()
        .filter(|path| !seen.contains(*path))
        .cloned()
        .collect();
    for paths in absent.chunks(64) {
        let revision = index.remove(paths, stop).await?;
        for path in paths {
            records.entries.remove(path);
        }
        records.advance(revision);
    }
    Ok(())
}

fn inspect(
    directory: &cap_std::fs::Dir,
    path: String,
    prior: Option<Prior>,
    resources: &resources::Resources,
    control: &crate::files::Control,
    stop: &CancellationToken,
) -> Result<Option<Parsed>, Fault> {
    let source = crate::files::index_text(
        directory,
        crate::files::Access::Restricted,
        &path,
        MAX_FILE_BYTES,
        control,
    )?;
    let Some(source) = source else {
        // A fresh size check proves the same omitted state. There are no
        // symbols or source revision to replace while the file is oversized.
        if prior.as_ref().is_some_and(|(_, rules, status)| {
            rules == &resources.fingerprint && *status == Status::TooLarge
        }) {
            return Ok(None);
        }
        return Ok(Some(Parsed::omitted(path, Status::TooLarge)));
    };
    let hash = blake3::hash(source.as_bytes()).to_hex().to_string();
    if prior.is_some_and(|(previous, rules, _)| {
        previous.as_deref() == Some(hash.as_str()) && rules == resources.fingerprint
    }) {
        return Ok(None);
    }
    resources.parser.parse(path, &source, stop).map(Some)
}

async fn commit(
    index: &index::Index,
    files: &[Parsed],
    fingerprint: &str,
    stop: &CancellationToken,
    deadline: Instant,
) -> Result<(usize, Vec<u64>), Fault> {
    let mut start = 0;
    let mut revisions = Vec::new();
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
        revisions.extend(index.replace(&files[start..end], fingerprint, stop).await?);
        start = end;
    }
    Ok((start, revisions))
}

//! Scan, bounded parallel reads/parses, then short writer transactions.
//! The staged approach follows ast-index v3.56.0 (MIT); see NOTICE.
use super::*;
use index::Cached;
use parser::Parsed;

#[derive(Default)]
pub(super) struct Report {
    pub visited: usize,
    pub skipped: usize,
    pub partial: bool,
    pub truncated: bool,
    pub issues: Vec<Issue>,
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
        let permit = tokio::select! {biased;_=stop.cancelled()=>return Err(cancelled()),permit=self.parsing.clone().acquire_owned()=>permit.map_err(error)?};
        let root = worktree.root.clone();
        let stop = stop.clone();
        let node_stop = worktree.stop.clone();
        let cached = cached.map(|cached| (cached.hash.clone(), cached.fingerprint.clone()));
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            check(&stop)?;
            check(&node_stop)?;
            let control = crate::files::Control::until(stop.clone(), deadline);
            let source = crate::files::index_text(root.as_path(), &path, MAX_FILE_BYTES, &control)?;
            let Some(source) = source else {
                return Ok(Some(Parsed::omitted(path, Status::TooLarge)));
            };
            let hash = blake3::hash(source.as_bytes()).to_hex().to_string();
            if cached.is_some_and(|(previous, rules)| {
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
        limit: usize,
    ) -> Result<Report, Fault> {
        check(stop)?;
        let generation = worktree.dirty.lock().unwrap().generation;
        let deadline = Instant::now() + Duration::from_secs(110);
        let root = worktree.root.clone();
        let scan_stop = stop.clone();
        let listing = tokio::task::spawn_blocking(move || {
            let control = crate::files::Control::until(scan_stop, deadline);
            crate::files::walk::collect_bounded(root.as_path(), "", limit, &control)
        })
        .await
        .map_err(error)??;
        let mut report = Report {
            visited: listing.report.visited,
            skipped: listing.report.skipped,
            partial: listing.report.partial,
            truncated: listing.report.truncated,
            issues: Vec::new(),
        };
        let seen: BTreeSet<_> = listing
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        let files = index.files().await?;
        let candidates: Vec<_> = listing
            .entries
            .into_iter()
            .filter(|entry| parser::candidate(&entry.path))
            .collect();
        for batch in candidates.chunks(BATCH) {
            check(stop)?;
            check(&worktree.stop)?;
            // Poll all reads in the batch together; a shared semaphore bounds CPU work.
            let parsed = futures::future::join_all(batch.iter().map(|entry| async {
                let result = self
                    .read(
                        worktree,
                        entry.path.clone(),
                        files.get(&entry.path),
                        resources.clone(),
                        stop,
                        deadline,
                    )
                    .await;
                (entry.path.clone(), result)
            }))
            .await;
            let mut changes = Vec::new();
            for (path, result) in parsed {
                match result {
                    Ok(Some(file)) => changes.push(file),
                    Ok(None) => {
                        if let Some(cached) = files.get(&path) {
                            report.partial |= cached.state.partial;
                            report.truncated |= cached.state.truncated;
                            if cached.state.status != Status::Parsed && report.issues.len() < 100 {
                                report.issues.push(Issue {
                                    path,
                                    status: cached.state.status,
                                });
                            }
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
                    Err(_) => {
                        changes.push(Parsed::omitted(path, Status::Invalid));
                    }
                }
            }
            for file in &changes {
                report.partial |= file.partial;
                report.truncated |= file.truncated;
                if file.status != Status::Parsed {
                    report.skipped += 1;
                    if report.issues.len() < 100 {
                        report.issues.push(Issue {
                            path: file.path.clone(),
                            status: file.status,
                        });
                    }
                }
            }
            // Parse tasks have finished and released every CPU permit before taking a writer.
            #[cfg(test)]
            {
                let pause = worktree.prepared.lock().unwrap().take();
                if let Some((entered, resume)) = pause {
                    let _ = entered.send(());
                    let _ = resume.await;
                }
            }
            index
                .replace(&changes, &resources.fingerprint, stop)
                .await?;
        }
        check(stop)?;
        // Only a complete walk establishes absence. File failures never turn unvisited
        // paths into deletions, and cancellation cannot reach this pruning branch.
        let current = worktree.dirty.lock().unwrap().generation == generation;
        if !listing.report.partial && !listing.report.truncated && current {
            index.prune(&seen, stop).await?;
        }
        let mut dirty = worktree.dirty.lock().unwrap();
        report.partial |= dirty.watch_failed;
        if !listing.report.partial && !listing.report.truncated {
            report.partial |= !dirty.finish(generation);
        }
        Ok(report)
    }
}

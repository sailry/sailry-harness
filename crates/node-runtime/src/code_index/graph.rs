//! Callers, callees, impact and a ranked map over name-resolved references.
//! Ideas follow Aider's repository map and name-based tag graphs; the code is
//! original. Results are heuristic and carry the confidence of each match.
use super::*;

mod rank;
mod resolve;
#[cfg(test)]
mod tests;
mod views;
pub(super) use resolve::{Candidate, definition};
pub(super) use views::run;

const MAX_NAME_BYTES: usize = 512;

impl Service {
    pub(crate) async fn graph(
        &self,
        id: WorktreeId,
        root: PathBuf,
        query: &Graph,
        stop: &CancellationToken,
    ) -> Result<GraphPage, Fault> {
        validate(query)?;
        let worktree = self.worktree(id, root)?;
        let running = stop.child_token();
        let _cancel = running.clone().drop_guard();
        let stop = &running;
        let operation = async {
            let _access = worktree.access.read().await;
            check(&worktree.stop)?;
            worktree
                .observe(self.watches.clone(), self.node, id, stop)
                .await?;
            let index = worktree.index().await?;
            let report = match worktree.sync.try_lock() {
                Ok(mut progress) => {
                    self.synchronize(
                        &worktree,
                        index,
                        self.resources().await?,
                        stop,
                        &mut progress,
                        sync::Limits::default(),
                    )
                    .await?
                }
                // Another reader is refreshing; its committed transactions stay readable.
                Err(_) => sync::Report::interrupted(),
            };
            let scoped = report.scoped(None);
            let mut page = index.graph(query, stop).await?;
            page.visited = report.visited;
            page.skipped = scoped.skipped;
            page.partial |= scoped.partial;
            page.truncated |= scoped.truncated;
            page.issues = scoped.issues;
            Ok(page)
        };
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err(cancelled()),
            _ = worktree.stop.cancelled() => Err(cancelled()),
            result = operation => result,
        }
    }
}

fn validate(query: &Graph) -> Result<(), Fault> {
    let invalid = || Fault::invalid("invalid code graph query");
    if query.limit == 0
        || query.limit > MAX_GRAPH_ITEMS
        || query.offset > MAX_GRAPH_OFFSET
        || query.depth == 0
        || query.depth > MAX_GRAPH_DEPTH
        || query.paths.len() > MAX_GRAPH_PATHS
        || query
            .name
            .as_ref()
            .is_some_and(|name| name.trim().is_empty() || name.len() > MAX_NAME_BYTES)
        || (query.line.is_some() && query.paths.len() != 1)
    {
        return Err(invalid());
    }
    for path in &query.paths {
        crate::files::path::entry_components(path)?;
    }
    let selected = query.name.is_some() || query.line.is_some();
    match query.view {
        View::Callers | View::Callees | View::Context if !selected => Err(invalid()),
        View::Impact if !selected && query.paths.is_empty() => Err(invalid()),
        _ => Ok(()),
    }
}

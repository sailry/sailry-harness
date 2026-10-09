//! Plugin semantic recall adapts the shared index back to current private entries.
use super::*;
use crate::store::knowledge::semantic::{Plan, Selection, Stamp};
use sailry_protocol::{Command, plugin::storage};

#[cfg(test)]
mod tests;

impl Service {
    pub(crate) async fn search_storage(
        &self,
        request: &Request,
        cancel: &CancellationToken,
    ) -> Result<storage::SearchPage, Fault> {
        let Command::SearchPluginValues(query) = &request.command else {
            return Err(Fault::invalid("plugin storage search expected"));
        };
        let ingress = self.ingress.upgrade().ok_or_else(unavailable)?;
        let plan = ingress.semantic_plan(request, cancel).await?;
        if plan.changed {
            self.refresh();
        }
        let original = query
            .query
            .as_deref()
            .filter(|text| !text.trim().is_empty());
        let (hits, indexing, available) = if let Some(original) = original
            && query.mode != Mode::Keyword
            && !plan.collections.is_empty()
        {
            let retrieval = Query {
                query: original.into(),
                collections: plan
                    .collections
                    .iter()
                    .map(|stored| stored.collection.id)
                    .collect(),
                mode: Mode::Semantic,
                tags: query.all.clone(),
                path: None,
                modified_after_ms: None,
                modified_before_ms: None,
                limit: DEFAULT_LIMIT,
                offset: 0,
            };
            self.semantic_candidates(request, &retrieval, &query.any, cancel)
                .await?
        } else {
            (vec![], false, false)
        };
        let indexing = indexing
            || plan.collections.iter().any(|stored| {
                matches!(
                    stored.collection.progress.status,
                    Status::Pending | Status::Indexing
                )
            });
        let selected = select(query, &plan, hits, indexing, available);
        let materialized = ingress
            .semantic_page(request, plan, selected, cancel)
            .await?;
        if materialized.stale {
            self.refresh();
        }
        Ok(materialized.page)
    }

    async fn semantic_candidates(
        &self,
        request: &Request,
        query: &Query,
        any: &[String],
        cancel: &CancellationToken,
    ) -> Result<(Vec<Hit>, bool, bool), Fault> {
        let Prepared {
            bindings,
            embeddings,
            indexing,
        } = self.prepare(request, query, cancel).await?;
        let available = !embeddings.is_empty();
        let path = self.path.clone();
        let query = query.clone();
        let any = any.to_vec();
        let cancel = cancel.clone();
        let hits = tokio::task::spawn_blocking(move || {
            index::Index::reader(&path)?.candidates(&query, &bindings, &embeddings, &any, &cancel)
        })
        .await
        .map_err(|_| unavailable())??;
        Ok((hits, indexing, available))
    }
}

fn select(
    query: &storage::Search,
    plan: &Plan,
    hits: Vec<Hit>,
    indexing: bool,
    available: bool,
) -> Selection {
    let mut semantic = Vec::new();
    let mut stamps = Vec::new();
    let mut seen = BTreeSet::new();
    for hit in hits {
        if plan.eligible.contains(&hit.citation.path) && seen.insert(hit.citation.path.clone()) {
            semantic.push(hit.citation.path.clone());
            stamps.push(Stamp {
                collection: hit.citation.collection,
                key: hit.citation.path,
                revision: hit.citation.revision,
            });
        }
    }
    let keyword = plan
        .keyword
        .iter()
        .filter(|(key, _)| {
            query.mode != Mode::Semantic
                || indexing
                || !available
                || semantic.is_empty()
                || !plan.eligible.contains(key)
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    let mode = if semantic.is_empty() {
        Mode::Keyword
    } else if keyword.is_empty() {
        Mode::Semantic
    } else {
        Mode::Hybrid
    };
    let keys = if mode == Mode::Semantic {
        semantic
    } else if mode == Mode::Keyword {
        keyword
    } else {
        rank::fuse(&keyword, &semantic)
            .into_iter()
            .map(|(key, _)| key)
            .collect()
    };
    Selection {
        keys,
        stamps,
        mode,
        indexing,
    }
}

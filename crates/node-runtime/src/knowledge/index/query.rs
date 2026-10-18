use super::*;
use sailry_link::CancellationToken;
use std::collections::BTreeMap;

const FILTER: &str = "JOIN sources s ON s.id=c.source JOIN collections k ON k.id=s.collection JOIN json_each(?1) admitted ON json_extract(admitted.value,'$.id')=k.id AND json_extract(admitted.value,'$.scope')=lower(hex(k.scope)) AND json_extract(admitted.value,'$.rebuild')=k.rebuild AND (json_extract(admitted.value,'$.generation')=k.generation OR (json_extract(admitted.value,'$.semantic')=0 AND k.generation<=json_extract(admitted.value,'$.generation'))) WHERE NOT EXISTS(SELECT 1 FROM json_each(?2) wanted WHERE NOT EXISTS(SELECT 1 FROM json_each(s.tags) actual WHERE actual.value=wanted.value)) AND (?3 IS NULL OR substr(s.locator,1,length(?3))=?3) AND (?4 IS NULL OR s.modified_ms>=?4) AND (?5 IS NULL OR s.modified_ms<=?5)";

impl Index {
    pub(in crate::knowledge) fn search(
        &self,
        query: &Query,
        allowed: &[Binding],
        embeddings: &[(Model, Vec<f32>)],
        indexing: bool,
        cancel: &CancellationToken,
    ) -> Result<Page, Fault> {
        let (ranked, mode) = self.ranks(query, allowed, embeddings, &[], cancel)?;
        let offset = query.offset as usize;
        let limit = query.limit.min(PAGE_LIMIT);
        let next_offset = if offset.saturating_add(usize::from(limit)) < ranked.len() {
            query.offset.checked_add(u32::from(limit))
        } else {
            None
        };
        let mut hits = self.hits(ranked.into_iter().skip(offset).take(limit.into()))?;
        for hit in &mut hits {
            hit.stale = indexing;
        }
        Ok(Page {
            hits,
            next_offset,
            mode,
            indexing,
            model: (mode != Mode::Keyword && embeddings.len() == 1).then(|| embeddings[0].0),
            untrusted: true,
        })
    }
    pub(in crate::knowledge) fn candidates(
        &self,
        query: &Query,
        allowed: &[Binding],
        embeddings: &[(Model, Vec<f32>)],
        any: &[String],
        cancel: &CancellationToken,
    ) -> Result<Vec<Hit>, Fault> {
        let mut query = query.clone();
        query.mode = Mode::Semantic;
        let (ranked, mode) = self.ranks(&query, allowed, embeddings, any, cancel)?;
        if mode == Mode::Keyword {
            return Ok(Vec::new());
        }
        self.hits(ranked.into_iter().take(CANDIDATES))
    }

    fn ranks(
        &self,
        query: &Query,
        allowed: &[Binding],
        embeddings: &[(Model, Vec<f32>)],
        any: &[String],
        cancel: &CancellationToken,
    ) -> Result<(Vec<(String, f32)>, Mode), Fault> {
        query.validate()?;
        if cancel.is_cancelled() {
            return Err(Fault::new(
                ErrorCode::Cancelled,
                "knowledge search was cancelled",
            ));
        }
        let admitted = allowed
            .iter()
            .filter(|binding| {
                query.collections.is_empty() || query.collections.contains(&binding.id)
            })
            .map(|binding| {
                Ok(serde_json::json!({
                    "id": binding.id,
                    "generation": integer(binding.generation)?,
                    "scope": blake3::Hash::from(binding.scope).to_hex().to_string(),
                    "rebuild": integer(binding.rebuild)?,
                    "semantic": !binding.paused,
                }))
            })
            .collect::<Result<Vec<_>, Fault>>()?;
        let admitted = serde_json::to_string(&admitted).map_err(error)?;
        let tags = serde_json::to_string(&query.tags).map_err(error)?;
        let any = serde_json::to_string(any).map_err(error)?;
        let modified_after = query.modified_after_ms.map(integer).transpose()?;
        let modified_before = query.modified_before_ms.map(integer).transpose()?;
        let mut keyword = Vec::new();
        let mut scores = BTreeMap::new();
        let mut statement = self.db.prepare(&format!(
            "SELECT c.key,bm25(chunks_fts,2,1,2,1) FROM chunks_fts JOIN chunks c ON c.id=chunks_fts.rowid {FILTER} AND (json_array_length(?9)=0 OR EXISTS(SELECT 1 FROM json_each(?9) wanted JOIN json_each(s.tags) actual ON actual.value=wanted.value)) AND chunks_fts MATCH ?6 ORDER BY bm25(chunks_fts,2,1,2,1),c.key LIMIT {CANDIDATES}",
        )).map_err(error)?;
        let rows = statement
            .query_map(
                params![
                    admitted,
                    tags,
                    query.path,
                    modified_after,
                    modified_before,
                    text::query(&query.query),
                    Option::<String>::None,
                    Option::<String>::None,
                    any
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, f32>(1)?)),
            )
            .map_err(error)?;
        for row in rows {
            if cancel.is_cancelled() {
                return Err(Fault::new(
                    ErrorCode::Cancelled,
                    "knowledge search was cancelled",
                ));
            }
            let (key, score) = row.map_err(error)?;
            keyword.push(key.clone());
            scores.insert(key, -score);
        }
        let mut semantic = Vec::new();
        let mut candidates = BinaryHeap::new();
        for (model, values) in embeddings.iter().filter(|_| query.mode != Mode::Keyword) {
            let mut statement = self.db.prepare(&format!(
                "SELECT c.key,c.vector FROM chunks c {FILTER} AND json_extract(admitted.value,'$.semantic')=1 AND (json_array_length(?9)=0 OR EXISTS(SELECT 1 FROM json_each(?9) wanted JOIN json_each(s.tags) actual ON actual.value=wanted.value)) AND k.model=?7 AND k.dimension=?8 AND c.vector IS NOT NULL",
            )).map_err(error)?;
            let rows = statement
                .query_map(
                    params![
                        admitted,
                        tags,
                        query.path,
                        modified_after,
                        modified_before,
                        Option::<String>::None,
                        model.key(),
                        integer(values.len())?,
                        any
                    ],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
                )
                .map_err(error)?;
            for row in rows {
                if cancel.is_cancelled() {
                    return Err(Fault::new(
                        ErrorCode::Cancelled,
                        "knowledge search was cancelled",
                    ));
                }
                let (key, bytes) = row.map_err(error)?;
                let score = Vector::decode(&bytes, values.len())?.cosine(values)?;
                retain(&mut candidates, Scored { key, score });
            }
        }
        if !candidates.is_empty() {
            let mut candidates = candidates
                .into_iter()
                .map(|candidate| candidate.0)
                .collect::<Vec<_>>();
            candidates.sort_unstable_by(|left, right| right.cmp(left));
            for candidate in candidates {
                semantic.push(candidate.key.clone());
                if query.mode == Mode::Semantic {
                    scores.insert(candidate.key, candidate.score);
                }
            }
        }
        let mode = if semantic.is_empty() {
            Mode::Keyword
        } else {
            query.mode
        };
        let ranked = match mode {
            Mode::Hybrid => rank::fuse(&keyword, &semantic),
            Mode::Semantic => semantic
                .into_iter()
                .map(|key| {
                    let score = scores[&key];
                    (key, score)
                })
                .collect(),
            Mode::Keyword => keyword
                .into_iter()
                .map(|key| {
                    let score = scores[&key];
                    (key, score)
                })
                .collect(),
        };
        Ok((ranked, mode))
    }

    fn hits(&self, ranked: impl Iterator<Item = (String, f32)>) -> Result<Vec<Hit>, Fault> {
        let mut statement = self.db.prepare(
            "SELECT c.title,c.text,s.collection,s.revision,s.locator,c.position FROM chunks c JOIN sources s ON s.id=c.source WHERE c.key=?1",
        ).map_err(error)?;
        let mut hits = Vec::new();
        for (key, score) in ranked {
            let hit = statement
                .query_row([&key], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                })
                .map_err(error)?;
            let mut citation = Citation {
                collection: hit.2.parse().map_err(error)?,
                chunk: key,
                revision: hit.3,
                path: hit.4,
                position: serde_json::from_str(&hit.5).map_err(error)?,
                link: String::new(),
            };
            citation.link = citation.url();
            hits.push(Hit {
                title: hit.0,
                text: hit.1,
                score: (score.max(0.0) * 1_000_000.0).round() as u32,
                citation,
                stale: false,
            });
        }
        Ok(hits)
    }
}

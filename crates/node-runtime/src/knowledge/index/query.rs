use super::*;
use sailry_link::CancellationToken;
use std::collections::BTreeMap;

const FILTER: &str = "JOIN sources s ON s.id=c.source JOIN collections k ON k.id=s.collection JOIN json_each(?1) admitted ON json_extract(admitted.value,'$.id')=k.id AND json_extract(admitted.value,'$.scope')=lower(hex(k.scope)) AND json_extract(admitted.value,'$.rebuild')=k.rebuild AND (json_extract(admitted.value,'$.generation')=k.generation OR (json_extract(admitted.value,'$.semantic')=0 AND k.generation<=json_extract(admitted.value,'$.generation'))) WHERE NOT EXISTS(SELECT 1 FROM json_each(?2) wanted WHERE NOT EXISTS(SELECT 1 FROM json_each(s.tags) actual WHERE actual.value=wanted.value)) AND (?3 IS NULL OR substr(s.locator,1,length(?3))=?3) AND (?4 IS NULL OR s.modified_ms>=?4) AND (?5 IS NULL OR s.modified_ms<=?5)";

impl Index {
    pub(in crate::knowledge) async fn search(
        &self,
        query: &Query,
        allowed: &[Binding],
        embeddings: &[(String, Vec<f32>)],
        indexing: bool,
        cancel: &CancellationToken,
    ) -> Result<Page, Fault> {
        let mut lease = self.lease(true).await?;
        let connection = lease.connection_mut();
        let tx = connection.transaction().await.map_err(error)?;
        let (ranked, mode) = ranks(&tx, query, allowed, embeddings, &[], cancel).await?;
        let offset = query.offset as usize;
        let limit = query.limit.min(PAGE_LIMIT);
        let next_offset = if offset.saturating_add(usize::from(limit)) < ranked.len() {
            query.offset.checked_add(u32::from(limit))
        } else {
            None
        };
        let mut hits = hits(&tx, ranked.into_iter().skip(offset).take(limit.into())).await?;
        for hit in &mut hits {
            hit.stale = indexing;
        }
        tx.commit().await.map_err(error)?;
        Ok(Page {
            hits,
            next_offset,
            mode,
            indexing,
            model: (mode != Mode::Keyword && embeddings.len() == 1)
                .then(|| embeddings[0].0.clone()),
            untrusted: true,
        })
    }

    pub(in crate::knowledge) async fn candidates(
        &self,
        query: &Query,
        allowed: &[Binding],
        embeddings: &[(String, Vec<f32>)],
        any: &[String],
        cancel: &CancellationToken,
    ) -> Result<Vec<Hit>, Fault> {
        let mut lease = self.lease(true).await?;
        let connection = lease.connection_mut();
        let tx = connection.transaction().await.map_err(error)?;
        let mut query = query.clone();
        query.mode = Mode::Semantic;
        let (ranked, mode) = ranks(&tx, &query, allowed, embeddings, any, cancel).await?;
        let hits = if mode == Mode::Keyword {
            Vec::new()
        } else {
            hits(&tx, ranked.into_iter().take(CANDIDATES)).await?
        };
        tx.commit().await.map_err(error)?;
        Ok(hits)
    }
}

async fn ranks(
    db: &Connection,
    query: &Query,
    allowed: &[Binding],
    embeddings: &[(String, Vec<f32>)],
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
    let admitted = allowed.iter().filter(|binding| query.collections.is_empty() || query.collections.contains(&binding.id)).map(|binding| {
        Ok(serde_json::json!({"id":binding.id,"generation":integer(binding.generation)?,"scope":blake3::Hash::from(binding.scope).to_hex().to_string(),"rebuild":integer(binding.rebuild)?,"semantic":!binding.paused}))
    }).collect::<Result<Vec<_>,Fault>>()?;
    let admitted = serde_json::to_string(&admitted).map_err(error)?;
    let tags = serde_json::to_string(&query.tags).map_err(error)?;
    let any = serde_json::to_string(any).map_err(error)?;
    let modified_after = query.modified_after_ms.map(integer).transpose()?;
    let modified_before = query.modified_before_ms.map(integer).transpose()?;
    let mut keyword = Vec::new();
    let mut scores = BTreeMap::new();
    let terms = text::query(&query.query);
    if !terms.is_empty() {
        let expression = ["field0", "field1", "cjk0", "cjk1"]
            .into_iter()
            .enumerate()
            .map(|(index, field)| {
                format!("{field}:({terms})^{}", if index % 2 == 0 { 2 } else { 1 })
            })
            .collect::<Vec<_>>()
            .join(" OR ");
        // Materialize the native scores before joins. Turso 0.8.3's direct
        // index scan can lose score order when a secondary sort is added.
        // All captured grants/source filters apply before the candidate limit.
        let mut rows = db.query(&format!("WITH candidates AS MATERIALIZED (SELECT id,fts_score(field0,field1,cjk0,cjk1,?6) AS score FROM chunks WHERE fts_match(field0,field1,cjk0,cjk1,?6)) SELECT c.key,f.score FROM candidates f JOIN chunks c ON c.id=f.id {FILTER} AND (json_array_length(?9)=0 OR EXISTS(SELECT 1 FROM json_each(?9) wanted JOIN json_each(s.tags) actual ON actual.value=wanted.value)) ORDER BY f.score DESC,c.key LIMIT {CANDIDATES}"),
            params![admitted.as_str(),tags.as_str(),query.path.as_deref(),modified_after,modified_before,expression,Option::<String>::None,Option::<String>::None,any.as_str()]).await.map_err(error)?;
        while let Some(row) = rows.next().await.map_err(error)? {
            if cancel.is_cancelled() {
                return Err(Fault::new(
                    ErrorCode::Cancelled,
                    "knowledge search was cancelled",
                ));
            }
            let key: String = row.get(0).map_err(error)?;
            let score = row.get::<f64>(1).map_err(error)? as f32;
            keyword.push(key.clone());
            scores.insert(key, score);
        }
    }
    let mut semantic = Vec::new();
    let mut candidates = BinaryHeap::new();
    for (model, values) in embeddings.iter().filter(|_| query.mode != Mode::Keyword) {
        let mut rows = db.query(&format!("SELECT c.key,c.vector FROM chunks c {FILTER} AND json_extract(admitted.value,'$.semantic')=1 AND (json_array_length(?9)=0 OR EXISTS(SELECT 1 FROM json_each(?9) wanted JOIN json_each(s.tags) actual ON actual.value=wanted.value)) AND k.model=?7 AND k.dimension=?8 AND c.vector IS NOT NULL"),
            params![admitted.as_str(),tags.as_str(),query.path.as_deref(),modified_after,modified_before,Option::<String>::None,model.as_str(),integer(values.len())?,any.as_str()]).await.map_err(error)?;
        while let Some(row) = rows.next().await.map_err(error)? {
            if cancel.is_cancelled() {
                return Err(Fault::new(
                    ErrorCode::Cancelled,
                    "knowledge search was cancelled",
                ));
            }
            let key = row.get(0).map_err(error)?;
            let bytes: Vec<u8> = row.get(1).map_err(error)?;
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

async fn hits(
    db: &Connection,
    ranked: impl Iterator<Item = (String, f32)>,
) -> Result<Vec<Hit>, Fault> {
    let mut statement = db.prepare("SELECT c.title,c.text,s.collection,s.revision,s.locator,c.position FROM chunks c JOIN sources s ON s.id=c.source WHERE c.key=?1").await.map_err(error)?;
    let mut hits = Vec::new();
    for (key, score) in ranked {
        let row = statement.query_row([key.as_str()]).await.map_err(error)?;
        let mut citation = Citation {
            collection: row
                .get::<String>(2)
                .map_err(error)?
                .parse()
                .map_err(error)?,
            chunk: key,
            revision: row.get(3).map_err(error)?,
            path: row.get(4).map_err(error)?,
            position: serde_json::from_str(&row.get::<String>(5).map_err(error)?).map_err(error)?,
            link: String::new(),
        };
        citation.link = citation.url();
        hits.push(Hit {
            title: row.get(0).map_err(error)?,
            text: row.get(1).map_err(error)?,
            score: (score.max(0.0) * 1_000_000.0).round() as u32,
            citation,
            stale: false,
        });
    }
    Ok(hits)
}

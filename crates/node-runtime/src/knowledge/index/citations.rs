use super::*;

pub(in crate::knowledge) struct Record {
    pub citation: Citation,
    pub title: String,
}

impl Index {
    pub(in crate::knowledge) async fn citation(
        &self,
        binding: &Binding,
        input: &CitationRequest,
    ) -> Result<Record, Fault> {
        let mut lease = self.lease(true).await?;
        let connection = lease.connection_mut();
        let tx = connection.transaction().await.map_err(error)?;
        if binding.id != input.collection || !readable(&tx, binding).await? {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "knowledge collection changed; refresh its results",
            ));
        }
        let row = match tx.prepare("SELECT c.title,s.locator,s.revision,c.position FROM chunks c JOIN sources s ON s.id=c.source WHERE s.collection=?1 AND c.key=?2").await.map_err(error)?.query_row(params![input.collection.to_string(), input.chunk.as_str()]).await {
            Ok(row) => row,
            Err(turso::Error::QueryReturnedNoRows) => return Err(Fault::new(ErrorCode::NotFound, "knowledge citation is no longer indexed")),
            Err(failure) => return Err(error(failure)),
        };
        let revision = row.get::<String>(2).map_err(error)?;
        if revision != input.revision {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "knowledge source changed; refresh its results",
            ));
        }
        let mut citation = Citation {
            collection: input.collection,
            chunk: input.chunk.clone(),
            revision,
            path: row.get(1).map_err(error)?,
            position: serde_json::from_str(&row.get::<String>(3).map_err(error)?).map_err(error)?,
            link: String::new(),
        };
        citation.link = citation.url();
        let record = Record {
            citation,
            title: row.get(0).map_err(error)?,
        };
        tx.commit().await.map_err(error)?;
        Ok(record)
    }
}

async fn readable(db: &Connection, binding: &Binding) -> Result<bool, Fault> {
    if !binding.paused {
        return current(db, binding).await;
    }
    db.prepare("SELECT EXISTS(SELECT 1 FROM collections WHERE id=?1 AND generation<=?2 AND scope=?3 AND rebuild=?4)").await.map_err(error)?
        .query_row(params![binding.id.to_string(), integer(binding.generation)?, binding.scope.as_slice(), integer(binding.rebuild)?]).await.map_err(error)?.get(0).map_err(error)
}

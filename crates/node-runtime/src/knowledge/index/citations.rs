use super::*;

pub(in crate::knowledge) struct Record {
    pub citation: Citation,
    pub title: String,
}

impl Index {
    pub(in crate::knowledge) fn citation(
        &self,
        binding: Binding,
        input: &CitationRequest,
    ) -> Result<Record, Fault> {
        if binding.id != input.collection || !self.readable(binding)? {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "knowledge collection changed; refresh its results",
            ));
        }
        let row = self.db.query_row(
            "SELECT c.title,s.locator,s.revision,c.position FROM chunks c JOIN sources s ON s.id=c.source WHERE s.collection=?1 AND c.key=?2",
            params![input.collection.to_string(), input.chunk],
            |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?)),
        ).optional().map_err(error)?;
        let Some((title, path, revision, position)) = row else {
            return Err(Fault::new(
                ErrorCode::NotFound,
                "knowledge citation is no longer indexed",
            ));
        };
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
            path,
            position: serde_json::from_str(&position).map_err(error)?,
            link: String::new(),
        };
        citation.link = citation.url();
        Ok(Record { citation, title })
    }

    fn readable(&self, binding: Binding) -> Result<bool, Fault> {
        if !binding.paused {
            return self.current(binding);
        }
        // Pausing retains text before the worker binds the new generation; source identity must match.
        self.db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM collections WHERE id=?1 AND generation<=?2 AND scope=?3 AND rebuild=?4)",
                params![
                    binding.id.to_string(),
                    integer(binding.generation)?,
                    binding.scope.as_slice(),
                    integer(binding.rebuild)?,
                ],
                |row| row.get(0),
            )
            .map_err(error)
    }
}

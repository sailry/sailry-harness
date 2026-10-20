use super::*;
use crate::knowledge::source::Stat;

impl Index {
    pub(in crate::knowledge) fn cached(
        &self,
        binding: &Binding,
        locator: &str,
        stat: Stat,
        tags: &[String],
        embedded: bool,
    ) -> Result<bool, Fault> {
        if !self.current(binding)? {
            return Ok(false);
        }
        // Size and modification time avoid repeated reads for ordinary writers.
        // A writer that preserves both can evade this witness; citations reread and hash.
        self.db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sources s JOIN file_stamps f ON f.source=s.id
             WHERE s.collection=?1 AND s.locator=?2 AND f.size=?3 AND f.modified_seconds=?4
             AND f.modified_nanos=?5 AND s.tags=?6 AND (?7=0 OR s.embedded=1) AND f.extractor=?8)",
                params![
                    binding.id.to_string(),
                    locator,
                    integer(stat.size)?,
                    integer(stat.modified_seconds)?,
                    stat.modified_nanos,
                    serde_json::to_string(tags).map_err(error)?,
                    embedded,
                    stat.extractor.as_slice()
                ],
                |row| row.get(0),
            )
            .map_err(error)
    }

    pub(in crate::knowledge) fn remember(
        &self,
        binding: &Binding,
        locator: &str,
        stat: Stat,
    ) -> Result<(), Fault> {
        if !self.current(binding)? {
            return Err(Fault::new(
                ErrorCode::Cancelled,
                "knowledge collection changed",
            ));
        }
        self.db
            .execute(
                "INSERT INTO file_stamps(source,size,modified_seconds,modified_nanos,extractor)
             SELECT id,?3,?4,?5,?6 FROM sources WHERE collection=?1 AND locator=?2
             ON CONFLICT(source) DO UPDATE SET size=excluded.size,
             modified_seconds=excluded.modified_seconds,modified_nanos=excluded.modified_nanos,extractor=excluded.extractor",
                params![
                    binding.id.to_string(),
                    locator,
                    integer(stat.size)?,
                    integer(stat.modified_seconds)?,
                    stat.modified_nanos,
                    stat.extractor.as_slice()
                ],
            )
            .map_err(error)?;
        Ok(())
    }
}

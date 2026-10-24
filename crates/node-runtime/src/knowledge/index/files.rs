use super::*;
use crate::knowledge::source::Stat;

impl Index {
    pub(in crate::knowledge) async fn cached(
        &self,
        binding: &Binding,
        locator: &str,
        stat: Stat,
        tags: &[String],
        embedded: bool,
    ) -> Result<bool, Fault> {
        let mut lease = self.lease(true).await?;
        let connection = lease.connection_mut();
        let tx = connection.transaction().await.map_err(error)?;
        if !current(&tx, binding).await? {
            return Ok(false);
        }
        // Ordinary writers are witnessed by size/time; citation reads still hash.
        let found = tx.prepare("SELECT EXISTS(SELECT 1 FROM sources s JOIN file_stamps f ON f.source=s.id WHERE s.collection=?1 AND s.locator=?2 AND f.size=?3 AND f.modified_seconds=?4 AND f.modified_nanos=?5 AND s.tags=?6 AND (?7=0 OR s.embedded=1) AND f.extractor=?8)").await.map_err(error)?
            .query_row(params![binding.id.to_string(), locator, integer(stat.size)?, integer(stat.modified_seconds)?, stat.modified_nanos, serde_json::to_string(tags).map_err(error)?, embedded, stat.extractor.as_slice()]).await.map_err(error)?.get(0).map_err(error)?;
        tx.commit().await.map_err(error)?;
        Ok(found)
    }

    pub(in crate::knowledge) async fn remember(
        &self,
        binding: &Binding,
        locator: &str,
        stat: Stat,
    ) -> Result<(), Fault> {
        let mut lease = self.lease(false).await?;
        let connection = lease.connection_mut();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Concurrent)
            .await
            .map_err(error)?;
        if !current(&tx, binding).await? {
            return Err(Fault::new(
                ErrorCode::Cancelled,
                "knowledge collection changed",
            ));
        }
        tx.execute("INSERT INTO file_stamps(source,size,modified_seconds,modified_nanos,extractor) SELECT id,?3,?4,?5,?6 FROM sources WHERE collection=?1 AND locator=?2 ON CONFLICT(source) DO UPDATE SET size=excluded.size,modified_seconds=excluded.modified_seconds,modified_nanos=excluded.modified_nanos,extractor=excluded.extractor",
            params![binding.id.to_string(), locator, integer(stat.size)?, integer(stat.modified_seconds)?, stat.modified_nanos, stat.extractor.as_slice()]).await.map_err(error)?;
        tx.commit().await.map_err(error)
    }
}

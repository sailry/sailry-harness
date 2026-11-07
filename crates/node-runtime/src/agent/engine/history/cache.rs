use super::*;

#[derive(Default)]
pub(crate) struct Cache {
    messages: BTreeMap<(String, i64), Option<model::Message>>,
}

impl Cache {
    pub(crate) async fn project(
        &mut self,
        db: &Connection,
        sequence: i64,
        record: &Record,
    ) -> Result<Option<model::Message>, Fault> {
        let key = (record.id.clone(), sequence);
        if let Some(message) = self.messages.get(&key) {
            return Ok(message.clone());
        }
        let message = super::project(db, record).await?;
        self.messages.insert(key, message.clone());
        Ok(message)
    }
}

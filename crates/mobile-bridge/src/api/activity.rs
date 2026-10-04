//! Adapt the shared Client inbox and Node-owned plugin notification actions.
use super::*;
#[cfg(test)]
mod tests;

impl Controller {
    /// Read notifications received through this controller's active subscriptions.
    pub async fn notifications(&self) -> Result<String, String> {
        self.check()?;
        let inbox = self.inbox.lock().await;
        serde_json::to_string(&serde_json::json!({
            "notices": inbox.notices(),
            "unread": inbox.unread(),
        }))
        .map_err(error)
    }

    pub async fn mark_notification_read(&self, id: String) -> Result<(), String> {
        self.check()?;
        let id: sailry_client::activity::Id = serde_json::from_str(&id).map_err(error)?;
        if let sailry_client::activity::Target::Notification(notification) = id.target {
            let address = self
                .handle
                .peers()
                .await
                .map_err(error)?
                .into_iter()
                .find(|address| address.id.as_bytes() == &id.node.0)
                .ok_or_else(|| "notification Node is unavailable".to_owned())?;
            let client = sailry_client::Client::new(self.handle.remote(address));
            client
                .execute(
                    client.prepare(sailry_protocol::Command::MarkNotificationRead {
                        id: notification,
                    }),
                )
                .await
                .map_err(error)?;
        }
        self.inbox.lock().await.mark_read(id);
        Ok(())
    }

    pub async fn mark_notifications_read(&self) -> Result<(), String> {
        self.check()?;
        let ids: Vec<_> = self
            .inbox
            .lock()
            .await
            .notices()
            .iter()
            .filter(|notice| !notice.read)
            .map(|notice| notice.id)
            .collect();
        for id in ids {
            self.mark_notification_read(serde_json::to_string(&id).map_err(error)?)
                .await?;
        }
        Ok(())
    }
}

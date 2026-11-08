use super::*;

const MEDIA_BYTES: usize = 64 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct Cache {
    messages: BTreeMap<(String, i64), Option<model::Message>>,
    media_bytes: usize,
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
        let bytes = message.as_ref().map_or(0, media_bytes);
        if bytes <= MEDIA_BYTES.saturating_sub(self.media_bytes) {
            self.media_bytes += bytes;
            self.messages.insert(key, message.clone());
        }
        Ok(message)
    }
}

fn media_bytes(message: &model::Message) -> usize {
    use rmcp::model::{ContentBlock, ResourceContents};
    message
        .content
        .iter()
        .map(|content| match content {
            Content::Image {
                data: model::Image::Inline { data_base64 },
                ..
            }
            | Content::Document { data_base64, .. } => data_base64.len(),
            Content::ToolResult(response) => response.result.as_ref().map_or(0, |result| {
                result
                    .content
                    .iter()
                    .map(|content| match content {
                        ContentBlock::Image(image) => image.data.len(),
                        ContentBlock::Audio(audio) => audio.data.len(),
                        ContentBlock::Resource(resource) => match &resource.resource {
                            ResourceContents::BlobResourceContents { blob, .. } => blob.len(),
                            _ => 0,
                        },
                        _ => 0,
                    })
                    .sum()
            }),
            _ => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn skips_retaining_media_above_budget() {
        let (_database, db) = super::super::tests::database().await;
        let mut cache = Cache {
            media_bytes: MEDIA_BYTES - 4,
            ..Default::default()
        };
        let source = model::Message::user()
            .with_generated_id()
            .with_image("AQID", "image/png");
        let first = capture(&db, TurnId::new(), &source, &BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(
            cache.project(&db, 1, &first).await.unwrap().unwrap(),
            source
        );
        assert_eq!(cache.media_bytes, MEDIA_BYTES);
        let mut second = first.clone();
        second.id = "uncached-media".into();
        let projected = cache.project(&db, 2, &second).await.unwrap().unwrap();
        assert_eq!(projected.content, source.content);
        assert_eq!(cache.messages.len(), 1);
        let text = model::Message::user()
            .with_generated_id()
            .with_text("Still cached");
        let record = capture(&db, TurnId::new(), &text, &BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(cache.project(&db, 3, &record).await.unwrap(), Some(text));
        assert_eq!(cache.messages.len(), 2);
    }

    #[test]
    fn includes_tool_media_payloads() {
        let message = model::Message::user()
            .with_image("AQID", "image/png")
            .with_tool_response(
                "media",
                Ok(CallToolResult::success(vec![
                    rmcp::model::ContentBlock::image("AQID", "image/png"),
                    rmcp::model::ContentBlock::audio("AQID", "audio/wav"),
                    rmcp::model::ContentBlock::resource(rmcp::model::ResourceContents::blob(
                        "AQID",
                        "fixture://blob",
                    )),
                ])),
            );
        assert_eq!(media_bytes(&message), 16);
    }
}

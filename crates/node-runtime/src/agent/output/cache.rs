use super::*;

#[derive(Default)]
pub(in crate::agent) struct Cache {
    entries: Vec<Entry>,
    reads: files::Reads,
}

struct Entry {
    identity: Option<Identity>,
    projected: ToolResult,
}

struct Identity {
    message: String,
    call: String,
    revision: Value,
    visible: bool,
    images: usize,
    omitted: Option<Value>,
}

impl Identity {
    fn capture(
        message: Option<&str>,
        revision: Option<&Value>,
        visible: bool,
        result: &ToolResult,
    ) -> Option<Self> {
        let (images, omitted) = media(result);
        Some(Self {
            message: message?.into(),
            call: result.id.clone(),
            revision: revision?.clone(),
            visible,
            images,
            omitted: omitted.cloned(),
        })
    }

    fn matches(&self, message: &Message, result: &ToolResult) -> bool {
        let (images, omitted) = media(result);
        message.id.as_deref() == Some(self.message.as_str())
            && result.id == self.call
            && message.metadata.operation_note("tool_output", &result.id) == Some(&self.revision)
            && message.metadata.agent_visible == self.visible
            && images == self.images
            && omitted == self.omitted.as_ref()
    }
}

fn media(result: &ToolResult) -> (usize, Option<&Value>) {
    let Ok(result) = &result.result else {
        return (0, None);
    };
    (
        result
            .content
            .iter()
            .filter(|content| matches!(content, ContentBlock::Image(_)))
            .count(),
        result
            .structured_content
            .as_ref()
            .and_then(|value| value.get("omitted_screenshots")),
    )
}

impl Cache {
    pub(in crate::agent) fn project(&mut self, messages: &mut [Message]) {
        let mut sources = messages.iter().flat_map(|message| {
            message.content.iter().filter_map(move |content| {
                let Content::ToolResult(result) = content else {
                    return None;
                };
                Some((message, result))
            })
        });
        let unchanged = self.entries.iter().all(|entry| {
            sources.next().is_some_and(|(message, result)| {
                entry
                    .identity
                    .as_ref()
                    .is_some_and(|identity| identity.matches(message, result))
            })
        });
        if !unchanged {
            *self = Self::default();
        }
        let mut position = 0;
        for message in messages {
            for content in &mut message.content {
                let Content::ToolResult(response) = content else {
                    continue;
                };
                if let Some(entry) = self.entries.get(position) {
                    response.clone_from(&entry.projected);
                } else {
                    let revision = message.metadata.operation_note("tool_output", &response.id);
                    let identity = Identity::capture(
                        message.id.as_deref(),
                        revision,
                        message.metadata.agent_visible,
                        response,
                    );
                    response_projection(
                        response,
                        revision,
                        message.metadata.agent_visible,
                        &mut self.reads,
                    );
                    self.entries.push(Entry {
                        identity,
                        projected: response.clone(),
                    });
                }
                position += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests;

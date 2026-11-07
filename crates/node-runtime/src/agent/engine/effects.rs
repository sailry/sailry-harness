use crate::agent::model::{Message, Usage};
use serde_json::Value;

#[derive(Clone)]
pub(crate) enum ConversationEffect {
    AppendMessage(Message),
    PatchToolRequestMeta {
        tool_call_id: String,
        patch: Value,
    },
    SetMessageVisibility {
        message_id: String,
        user_visible: bool,
        agent_visible: bool,
    },
}

#[derive(Clone)]
pub(crate) enum Effect {
    Conversation(ConversationEffect),
    Usage(Usage),
    Compaction {
        summary: Box<Message>,
        usage: Usage,
        files: Vec<crate::agent::canonical::CompactedFile>,
        tools: Vec<String>,
    },
    Records(Vec<crate::agent::canonical::Record>),
    Approval {
        request: crate::agent::canonical::ToolIdentity,
        binding: Option<crate::store::agent::ApprovalBinding>,
    },
}

impl From<Message> for Effect {
    fn from(message: Message) -> Self {
        Self::Conversation(ConversationEffect::AppendMessage(message))
    }
}

impl From<ConversationEffect> for Effect {
    fn from(effect: ConversationEffect) -> Self {
        Self::Conversation(effect)
    }
}

impl Effect {
    pub(crate) fn identify(&mut self) {
        match self {
            Self::Conversation(ConversationEffect::AppendMessage(message)) => message.ensure_id(),
            Self::Compaction { summary, .. } => summary.ensure_id(),
            _ => {}
        }
    }
}

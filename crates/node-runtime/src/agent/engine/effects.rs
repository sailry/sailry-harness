use goose_agent::{
    inference::InferenceEffect,
    operation::{ConversationEffect, MachineEffect},
};
use goose_provider_types::conversation::{message::Message, token_usage::ProviderUsage};

pub(crate) enum Effect {
    Conversation(ConversationEffect),
    Usage(ProviderUsage),
    Compaction {
        summary: Message,
        usage: ProviderUsage,
    },
}

impl From<Message> for Effect {
    fn from(message: Message) -> Self {
        Self::Conversation(message.into())
    }
}

impl From<ConversationEffect> for Effect {
    fn from(effect: ConversationEffect) -> Self {
        Self::Conversation(effect)
    }
}

impl MachineEffect for Effect {
    fn ensure_message_ids(&mut self) {
        match self {
            Self::Conversation(effect) => effect.ensure_message_ids(),
            Self::Compaction { summary, .. } => {
                if summary.id.is_none() {
                    *summary = summary.clone().with_generated_id();
                }
            }
            Self::Usage(_) => {}
        }
    }
}

impl InferenceEffect for Effect {
    fn record_usage(usage: ProviderUsage) -> Self {
        Self::Usage(usage)
    }
}

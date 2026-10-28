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
        files: Vec<crate::agent::canonical::CompactedFile>,
    },
    Records(Vec<crate::agent::canonical::Record>),
    Approval {
        request: crate::agent::canonical::ToolIdentity,
        binding: Option<crate::store::agent::ApprovalBinding>,
    },
}

impl From<Message> for Effect {
    fn from(message: Message) -> Self {
        Self::Conversation(message.into())
    }
}

impl Clone for Effect {
    fn clone(&self) -> Self {
        match self {
            Self::Conversation(effect) => Self::Conversation(match effect {
                ConversationEffect::AppendMessage(message) => {
                    ConversationEffect::AppendMessage(message.clone())
                }
                ConversationEffect::ReplaceConversation(conversation) => {
                    ConversationEffect::ReplaceConversation(conversation.clone())
                }
                ConversationEffect::PatchToolRequestMeta {
                    tool_call_id,
                    patch,
                } => ConversationEffect::PatchToolRequestMeta {
                    tool_call_id: tool_call_id.clone(),
                    patch: patch.clone(),
                },
                ConversationEffect::SetMessageOperationNote {
                    message_id,
                    operation,
                    key,
                    value,
                } => ConversationEffect::SetMessageOperationNote {
                    message_id: message_id.clone(),
                    operation: operation.clone(),
                    key: key.clone(),
                    value: value.clone(),
                },
                ConversationEffect::SetMessageVisibility {
                    message_id,
                    user_visible,
                    agent_visible,
                } => ConversationEffect::SetMessageVisibility {
                    message_id: message_id.clone(),
                    user_visible: *user_visible,
                    agent_visible: *agent_visible,
                },
            }),
            Self::Usage(usage) => Self::Usage(usage.clone()),
            Self::Compaction {
                summary,
                usage,
                files,
            } => Self::Compaction {
                summary: summary.clone(),
                usage: usage.clone(),
                files: files.clone(),
            },
            Self::Records(records) => Self::Records(records.clone()),
            Self::Approval { request, binding } => Self::Approval {
                request: request.clone(),
                binding: binding.clone(),
            },
        }
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
            Self::Usage(_) | Self::Records(_) | Self::Approval { .. } => {}
        }
    }
}

impl InferenceEffect for Effect {
    fn record_usage(usage: ProviderUsage) -> Self {
        Self::Usage(usage)
    }
}

//! Serializable composer presentation only; restoring never sends a Node command.
use super::*;
use crate::content::images::Local;
use gpui_kit::base::input::{InlineToken, InputContent};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Token {
    pub range: std::ops::Range<usize>,
    pub id: String,
    pub text: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Identity {
    pub node: NodeId,
    pub session: Option<SessionId>,
    pub project: Option<ProjectId>,
    pub worktree: Option<WorktreeId>,
    pub resource: Option<sailry_protocol::connection::Resource>,
    pub assistant: Option<sailry_protocol::plugin::conversation::Binding>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Attachment {
    pub index: usize,
    pub value: sailry_protocol::attachment::Attachment,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Draft {
    pub identity: Identity,
    pub text: String,
    pub tokens: Vec<Token>,
    pub selection: std::ops::Range<usize>,
    pub references: Vec<sailry_protocol::conversation::reference::Reference>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    pub commands: BTreeMap<String, crate::plugins::contributions::Key>,
    pub configuration: Option<SessionConfig>,
    pub configuration_owner: NodeId,
    pub mode: Option<sailry_protocol::WorkMode>,
    pub permission: Option<sailry_protocol::Permission>,
}

#[derive(Clone)]
pub(crate) struct Snapshot {
    pub draft: Draft,
    pub sources: Vec<Local>,
}

impl View {
    pub(crate) fn recovery_identity(&self) -> Identity {
        Identity {
            node: self.binding.client.target(),
            session: self.session(),
            project: self.binding.project,
            worktree: self.binding.worktree,
            resource: self.resource,
            assistant: self.assistant.clone(),
        }
    }

    pub(crate) fn snapshot_draft(&self, cx: &App) -> Result<Option<Snapshot>, &'static str> {
        if self.busy() || !self.attachments.can_retarget() || self.queue.read(cx).update_pending() {
            return Err("updates_pending_request");
        }
        if self.editing.is_some() || self.queue.read(cx).editing_draft() {
            return Err("updates_finish_edit");
        }
        let input = self.input.read(cx);
        let text = input.value().to_string();
        if self.session.is_some()
            && text.is_empty()
            && self.references.selected.is_empty()
            && !self.has_attachments()
        {
            return Ok(None);
        }
        Ok(Some(Snapshot {
            draft: Draft {
                identity: self.recovery_identity(),
                text,
                tokens: input
                    .tokens()
                    .iter()
                    .map(|span| Token {
                        range: span.range(),
                        id: span.token().id().to_string(),
                        text: span.token().text().to_string(),
                        label: span.token().label().to_string(),
                    })
                    .collect(),
                selection: input.selected_range(),
                references: self.references.selected.clone(),
                attachments: self.captured_attachments(),
                commands: self.references.command_keys.clone(),
                configuration: self
                    .session
                    .is_none()
                    .then(|| self.config.clone())
                    .flatten(),
                configuration_owner: self.config_owner,
                mode: self.draft_mode,
                permission: self.draft_permission,
            },
            sources: self.attachment_sources(),
        }))
    }

    pub(crate) fn recover_draft(
        &mut self,
        snapshot: Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let count = snapshot.draft.attachments.len() + snapshot.sources.len();
        if self.recovery_identity() != snapshot.draft.identity
            || !self.connected()
            || self.busy()
            || !self.input.read(cx).value().is_empty()
            || !self.references.selected.is_empty()
            || self.has_attachments()
            || count > 8
            || snapshot
                .draft
                .attachments
                .iter()
                .enumerate()
                .any(|(index, attachment)| {
                    attachment.index >= count
                        || index > 0
                            && snapshot.draft.attachments[index - 1].index >= attachment.index
                })
        {
            return false;
        }
        let mut content = InputContent::new(snapshot.draft.text.clone());
        for token in &snapshot.draft.tokens {
            let Ok(next) = content.with_token(
                token.range.clone(),
                InlineToken::new(token.id.clone(), token.text.clone())
                    .with_label(token.label.clone()),
            ) else {
                return false;
            };
            content = next;
        }
        let selection = snapshot.draft.selection;
        if selection.start > selection.end
            || selection.end > snapshot.draft.text.len()
            || !snapshot.draft.text.is_char_boundary(selection.start)
            || !snapshot.draft.text.is_char_boundary(selection.end)
        {
            return false;
        }
        if self.session.is_none() {
            self.config = snapshot.draft.configuration;
            self.config_owner = snapshot.draft.configuration_owner;
            self.draft_mode = snapshot.draft.mode;
            self.draft_permission = snapshot.draft.permission;
        }
        // Existing sessions keep the authoritative configuration received from Node.
        self.references.selected = snapshot.draft.references;
        self.references.command_keys = snapshot.draft.commands;
        let mut sources = snapshot.sources.into_iter();
        let mut attachments = snapshot.draft.attachments.into_iter().peekable();
        for index in 0..count {
            if attachments
                .peek()
                .is_some_and(|attachment| attachment.index == index)
            {
                self.restore_attachments(vec![attachments.next().unwrap().value], cx);
            } else {
                self.restore_attachment_sources(vec![sources.next().unwrap()], cx);
            }
        }
        self.restored_input = Some(content.clone());
        self.input.update(cx, |input, cx| {
            input.set_value(content, window, cx);
            input.set_selected_range(selection, cx);
        });
        cx.notify();
        true
    }

    pub(super) fn recovered_input(&mut self, cx: &App) -> bool {
        let Some(content) = self.restored_input.as_ref() else {
            return false;
        };
        if &self.input.read(cx).content() == content {
            // Focus, caret, and selection also notify this entity. Keep restoration
            // passive until a real text edit, not only its first notification.
            true
        } else {
            self.restored_input = None;
            false
        }
    }
}

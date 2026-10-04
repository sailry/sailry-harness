mod activity;
pub(crate) mod approval;
mod composer;
mod context;
mod disclosure;
pub(crate) mod file_reference;
pub(crate) mod fixture;
mod highlights;
pub(crate) mod interaction;
mod layout;
pub(crate) mod live;
mod mode;
pub(crate) mod models;
mod permission;
pub(crate) mod queue;
pub(crate) mod references;
pub(crate) mod subagent;
pub(crate) mod surface;
pub(crate) mod transcript;
pub(crate) mod turn;
mod usage;
mod welcome;

pub(crate) use composer::Options;
pub(crate) use fixture::sample as sample_turn;
pub(crate) use layout::input_surface;

use crate::{shell::Shell, tr};
use gpui_kit::component::{message_scroller::MessageScroller, *};
use gpui_kit::*;

pub(crate) use crate::preview::CONTENT_WIDTH;
pub(crate) const MIN_WIDTH: f32 = 320.;

impl Shell {
    pub(crate) fn conversation(&self, cx: &mut Context<Self>) -> AnyElement {
        use gpui_kit::prelude::FluentBuilder as _;

        let key = (self.host, self.session);
        let conversation = &self.conversations[&key];
        let body = if conversation.turns.is_empty() {
            self.welcome(cx)
        } else {
            let owner = cx.entity().downgrade();
            MessageScroller::new(
                format!("conversation-scroll-{}-{}", key.0, key.1),
                conversation.scroller.clone(),
                move |index, _, cx| {
                    owner
                        .update(cx, |shell, cx| shell.turn_view(key, index, cx))
                        .unwrap_or_else(|_| div().into_any_element())
                },
            )
            .with_list_style(StyleRefinement::default().px_0().py_6())
            .with_jump_button_label(tr("turn_latest"))
            .with_jump_button_renderer(|button| {
                button
                    .without_tooltip()
                    .accessibility_label(tr("turn_latest"))
            })
            .into_any_element()
        };
        v_flex()
            .debug_selector(|| "conversation-page".into())
            .size_full()
            .min_h_0()
            .children(crate::theme::banner(
                if conversation.turns.is_empty() {
                    "banner.new_session"
                } else {
                    "banner.conversation.top"
                },
                cx,
            ))
            .child(div().flex_1().min_h_0().child(body))
            .children(
                (!conversation.turns.is_empty())
                    .then(|| crate::theme::banner("banner.conversation.bottom", cx))
                    .flatten(),
            )
            .when(!conversation.turns.is_empty(), |body| {
                body.child(self.composer(cx))
            })
            .into_any_element()
    }

    fn linked_message(
        &self,
        id: impl Into<ElementId>,
        text: SharedString,
        cx: &mut Context<Self>,
    ) -> crate::content::markdown::View {
        let owner = cx.entity().downgrade();
        crate::content::markdown::View::markdown(id, text).on_link_click(move |link, window, cx| {
            _ = owner.update(cx, |shell, cx| {
                shell.open_conversation_link(link.clone(), window, cx)
            });
        })
    }

    pub(crate) fn send_preview(
        &mut self,
        key: (usize, usize),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(conversation) = self.conversations.get_mut(&key) else {
            return;
        };
        let value = conversation.input.read(cx).value();
        // An Enter delivered during IME composition must not admit a preview turn.
        if conversation.input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            return;
        }
        if value.trim().is_empty()
            && !conversation.options.attachment
            && references::active(
                &conversation.input.read(cx).content(),
                &conversation.options.references,
            )
            .is_empty()
        {
            return;
        }
        if !self.validate_references(key, window, cx) {
            return;
        }
        let project = self.workspace.sessions[&key].owner.project;
        if !self.workspace.projects[&project].trusted {
            self.project_trust(
                project,
                Some(crate::workspace::Pending::Send(key)),
                window,
                cx,
            );
            return;
        }
        let options = self.queued_options(key, cx);
        let owner = self.workspace.sessions[&key].owner;
        let conversation = self.conversations.get_mut(&key).unwrap();
        conversation.queue.push(value, options, owner);
        conversation.options.attachment = false;
        conversation.options.references.clear();
        conversation.references.dismiss();
        conversation
            .input
            .update(cx, |input, cx| input.set_value("", window, cx));
        if self.begin_queued_preview(key, cx) {
            self.animate_preview(key, window, cx);
        }
        cx.notify();
    }

    pub(crate) fn stop_preview(&mut self, key: (usize, usize), cx: &mut Context<Self>) {
        if let Some(thread) = self.conversations.get_mut(&key) {
            thread.queue.paused = true;
        }
        if let Some(conversation) = self.conversations.get_mut(&key)
            && let Some(turn) = conversation
                .turns
                .last_mut()
                .filter(|turn| turn.status.active())
        {
            turn.stop(turn::Status::Cancelled);
            turn.finished_at = Some(tr("turn_preview_time"));
            conversation.preview_task = None;
            conversation.interaction_drafts.clear();
            conversation
                .scroller
                .update(cx, |state, cx| state.remeasure(cx));
            self.refresh_subagent_panel(key, cx);
            cx.notify();
        }
    }
}

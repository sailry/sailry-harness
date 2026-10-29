//! Parsed tool text survives virtual rows while its Client source stays cached.
use super::*;
use gpui_kit::component::text::{TextView, TextViewState};
use sailry_protocol::tool::detail::Detail;
use std::sync::{Arc, Weak};

pub(in crate::conversation::live) struct Parsed {
    turn: TurnId,
    source: Weak<Detail>,
    state: Entity<TextViewState>,
}

impl View {
    pub(in crate::conversation::live) fn prune_tool_texts(&self) {
        self.tool_texts
            .borrow_mut()
            .retain(|_, text| text.source.strong_count() > 0 && self.rows.contains(&text.turn));
    }

    pub(super) fn code(
        &self,
        id: &str,
        call: &Call,
        text: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        surface::scroll(
            format!("{id}-scroll"),
            self.literal_text(id, call, text, cx),
        )
    }

    pub(super) fn literal_text(
        &self,
        id: &str,
        call: &Call,
        text: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let size = px(f32::from(self.message_text_size));
        let Some(detail) = &call.detail else {
            return surface::literal_text(id, text, size, cx);
        };
        let source = surface::literal(text);
        let state = {
            let mut texts = self.tool_texts.borrow_mut();
            let parsed = texts.entry(id.to_owned()).or_insert_with(|| Parsed {
                turn: call.turn,
                source: Arc::downgrade(detail),
                state: cx.new(|cx| TextViewState::markdown(&source, cx)),
            });
            parsed.source = Arc::downgrade(detail);
            parsed.state.clone()
        };
        state.update(cx, |state, cx| state.set_text(&source, cx));
        surface::text_view(id, TextView::new(&state), size, cx)
    }
}

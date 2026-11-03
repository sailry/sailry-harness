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
        let (hidden, text) = if call
            .resolved
            .as_ref()
            .is_some_and(|resolved| resolved.command.is_some())
        {
            tail(text, COMMAND_LINES)
        } else {
            (0, text)
        };
        let body = self.parsed_text(id, call, text, size, cx);
        if hidden == 0 {
            return body;
        }
        v_flex()
            .w_full()
            .min_w_0()
            .child(
                surface::notice(
                    rust_i18n::t!("tool_output_earlier", count = hidden).to_string(),
                    cx,
                )
                .py_0()
                .pb_2(),
            )
            .child(body)
            .into_any_element()
    }

    fn parsed_text(
        &self,
        id: &str,
        call: &Call,
        text: &str,
        size: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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

/// Expanded command output shows its last lines, where results and errors
/// land. Stored history and model input keep the full output.
const COMMAND_LINES: usize = 100;

/// Returns the number of hidden leading lines and the visible tail.
fn tail(text: &str, limit: usize) -> (usize, &str) {
    let total = text.lines().count();
    if total <= limit {
        return (0, text);
    }
    let hidden = total - limit;
    let start = text
        .match_indices('\n')
        .nth(hidden - 1)
        .map_or(0, |(index, _)| index + 1);
    (hidden, &text[start..])
}

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_last_lines() {
        assert_eq!(super::tail("a\nb", 2), (0, "a\nb"));
        assert_eq!(super::tail("a\nb\nc\n", 2), (1, "b\nc\n"));
        assert_eq!(super::tail("a\nb\nc", 1), (2, "c"));
    }
}

use super::*;
use gpui_kit::component::input::Escape;
use sailry_protocol::conversation::{Entry, Part};

actions!(sailry, [StopTurn, FocusComposer, CycleMode, ModelControls]);

pub(crate) fn init(cx: &mut App) {
    for (key, action, context) in [
        ("escape", Escape.boxed_clone(), "LiveConversation"),
        ("secondary-.", StopTurn.boxed_clone(), "LiveConversation"),
        (
            "secondary-l",
            FocusComposer.boxed_clone(),
            "LiveConversation",
        ),
        ("shift-tab", CycleMode.boxed_clone(), "ConversationComposer"),
        (
            "secondary-shift-m",
            ModelControls.boxed_clone(),
            "LiveConversation",
        ),
    ] {
        cx.bind_keys([KeyBinding::load(
            key,
            action,
            Some(
                KeyBindingContextPredicate::parse(context)
                    .expect("static shortcut context")
                    .into(),
            ),
            false,
            None,
            cx,
        )
        .expect("static shortcut")]);
    }
}

#[derive(Debug, PartialEq, Eq)]
enum EscapeTarget {
    Composition,
    References,
    Selection,
    Turn,
    None,
}

fn escape_target(
    focused: bool,
    composing: bool,
    references: bool,
    selection: bool,
    running: bool,
) -> EscapeTarget {
    if !focused {
        EscapeTarget::None
    } else if composing {
        EscapeTarget::Composition
    } else if references {
        EscapeTarget::References
    } else if selection {
        EscapeTarget::Selection
    } else if running {
        EscapeTarget::Turn
    } else {
        EscapeTarget::None
    }
}

fn last_message(entries: &[Entry]) -> Option<String> {
    let entry = entries
        .iter()
        .filter(|entry| entry.author == "user")
        .max_by_key(|entry| entry.sequence)?;
    if entry
        .parts
        .iter()
        .any(|part| !matches!(part, Part::Text(_)))
    {
        return None;
    }
    let text: String = entry
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    (!text.is_empty()).then_some(text)
}

impl View {
    pub(super) fn escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        let composer = self.input.read(cx).focus_handle(cx).is_focused(window);
        let transcript = self.transcript_focus.is_focused(window);
        let composing = composer
            && self.input.update(cx, |input, cx| {
                input.marked_text_range(window, cx).is_some()
            });
        let selection = if composer {
            self.input.read(cx).selected_range()
        } else {
            0..0
        };
        match escape_target(
            composer || transcript,
            composing,
            composer && self.references.open,
            (composer && !selection.is_empty()) || self.selection.is_some(),
            !self.readonly() && self.active_run().is_some(),
        ) {
            EscapeTarget::Composition => {
                cx.propagate();
                return;
            }
            EscapeTarget::References => self.reference_action("escape", window, cx),
            EscapeTarget::Selection => {
                if composer && !selection.is_empty() {
                    self.input.update(cx, |input, cx| {
                        input.set_selected_range(selection.end..selection.end, cx)
                    });
                }
                self.dismiss_selection(cx);
            }
            EscapeTarget::Turn => self.stop(window, cx),
            EscapeTarget::None => {
                cx.propagate();
                return;
            }
        }
        cx.stop_propagation();
        window.prevent_default();
    }

    pub(super) fn stop_shortcut(
        &mut self,
        _: &StopTurn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.readonly() {
            self.stop(window, cx);
        }
    }

    pub(super) fn focus_composer(
        &mut self,
        _: &FocusComposer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus(window, cx);
    }

    pub(super) fn recall_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.references.open {
            self.reference_action("up", window, cx);
            return;
        }
        if self.readonly()
            || self.has_input(cx)
            || !self.input.read(cx).value().is_empty()
            || self.input.update(cx, |input, cx| {
                input.marked_text_range(window, cx).is_some()
            })
        {
            cx.propagate();
            return;
        }
        let Some(text) = self
            .history
            .snapshot
            .as_ref()
            .and_then(|snapshot| last_message(&snapshot.page.entries))
        else {
            cx.propagate();
            return;
        };
        self.input.update(cx, |input, cx| {
            let end = text.len();
            input.set_value(text, window, cx);
            input.set_selected_range(end..end, cx);
        });
        cx.stop_propagation();
        window.prevent_default();
    }

    pub(super) fn cycle_mode(
        &mut self,
        _: &CycleMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.input.read(cx).focus_handle(cx).is_focused(window)
            || self.input.update(cx, |input, cx| {
                input.marked_text_range(window, cx).is_some()
            })
        {
            cx.propagate();
            return;
        }
        cx.stop_propagation();
        window.prevent_default();
        let builtins = crate::conversation::mode::MODES;
        let plugins: Vec<_> = self
            .plugin_modes(cx)
            .into_iter()
            .filter(|mode| mode.enabled && self.config.is_some())
            .collect();
        let selected = self.selected_plugin_mode(cx);
        let index = selected
            .and_then(|selected| plugins.iter().position(|mode| mode.key() == selected.key()))
            .map(|index| builtins.len() + index)
            .unwrap_or_else(|| {
                builtins
                    .iter()
                    .position(|mode| *mode == self.current_mode())
                    .unwrap_or(0)
            });
        let next = (index + 1) % (builtins.len() + plugins.len());
        if let Some(mode) = builtins.get(next) {
            self.select_mode(*mode, window, cx);
        } else if let Some(mode) = plugins.get(next - builtins.len()) {
            self.select_plugin_mode(mode, window, cx);
        }
    }

    pub(super) fn open_models(
        &mut self,
        _: &ModelControls,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.readonly() || self.configuration_blocked() || !self.can_pick_model() {
            return;
        }
        if !self.sync_model_controls(cx) {
            cx.emit(Event::Settings(self.settings_node()));
            return;
        }
        self.model_controls
            .update(cx, |panel, cx| panel.request_open(cx));
        if self.compact_composer {
            self.composer_settings
                .update(cx, |panel, cx| panel.reset(cx));
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn escape_preserves_composition_and_selection() {
        assert_eq!(
            escape_target(true, true, true, true, true),
            EscapeTarget::Composition
        );
        assert_eq!(
            escape_target(true, false, true, true, true),
            EscapeTarget::References
        );
        assert_eq!(
            escape_target(true, false, false, true, true),
            EscapeTarget::Selection
        );
        assert_eq!(
            escape_target(true, false, false, false, true),
            EscapeTarget::Turn
        );
        assert_eq!(
            escape_target(true, false, false, false, false),
            EscapeTarget::None
        );
        assert_eq!(
            escape_target(false, true, true, true, true),
            EscapeTarget::None
        );
    }

    #[test]
    fn recalls_latest_user_text() {
        let entry = |sequence, author: &str, text: &str| Entry {
            sequence,
            id: sequence.to_string(),
            turn: TurnId::new(),
            author: author.into(),
            branch: String::new(),
            timestamp_ms: 0,
            parts: vec![Part::Text(text.into())],
            citations: Vec::new(),
            search_suggestions: None,
            usage: None,
        };
        let mut entries = [
            entry(1, "user", "Earlier"),
            entry(3, "user", "Latest"),
            entry(4, "assistant", "Answer"),
            entry(2, "tool", "Tool output"),
        ];
        assert_eq!(last_message(&entries).as_deref(), Some("Latest"));
        assert_eq!(last_message(&entries[2..]), None);
        assert_eq!(last_message(&[]), None);
        entries[1]
            .parts
            .push(Part::Resource(serde_json::json!({"fixture": "attachment"})));
        assert_eq!(last_message(&entries), None);
    }
}

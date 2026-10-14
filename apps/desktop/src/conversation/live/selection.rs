//! Selection actions operate on the captured excerpt, never the whole message.
use super::*;
use crate::content::selection::Excerpt;
use gpui_kit::{
    base::{Placement, Positioner},
    component::button::{Button, ButtonVariants},
};

impl View {
    pub(super) fn select_excerpt(&mut self, excerpt: Excerpt, cx: &mut Context<Self>) {
        if !excerpt.text.trim().is_empty() {
            self.selection = Some(excerpt);
            cx.notify();
        }
    }

    pub(super) fn dismiss_selection(&mut self, cx: &mut Context<Self>) {
        if self.selection.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn insert_quote(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.readonly()
            || self.input.update(cx, |input, cx| {
                input.marked_text_range(window, cx).is_some()
            })
        {
            return false;
        }
        let mut quote = String::new();
        for line in text.lines() {
            if !quote.is_empty() {
                quote.push('\n');
            }
            quote.push_str("> ");
            quote.push_str(line);
        }
        self.input.update(cx, |input, cx| {
            let end = input.value().len();
            input.set_selected_range(end..end, cx);
            let separator = if end > 0 { "\n\n" } else { "" };
            input.replace(format!("{separator}{quote}\n\n"), window, cx);
            input.focus(window, cx);
        });
        self.references.dismiss();
        self.dismiss_selection(cx);
        cx.notify();
        true
    }

    pub(super) fn selection_toolbar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let excerpt = self.selection.as_ref()?;
        Some(
            deferred(
                Positioner::side(excerpt.anchor)
                    .placement(Placement::Top)
                    .offset(px(8.))
                    .occlude()
                    .child(
                        h_flex()
                            .id("message-selection-actions")
                            .popover_style(cx)
                            .rounded_full_style(cx)
                            .p_1()
                            .gap_1()
                            .debug_selector(|| "message-selection-actions".into())
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_mouse_down_out(cx.listener(
                                |view, event: &MouseDownEvent, _, cx| {
                                    if event.button == MouseButton::Left {
                                        view.dismiss_selection(cx);
                                    }
                                },
                            ))
                            .child(
                                Button::new("selection-quote")
                                    .ghost()
                                    .small()
                                    .rounded_full_style(cx)
                                    .label(tr("message_quote"))
                                    .disabled(self.readonly())
                                    .debug_selector(|| "message-selection-quote".into())
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        if let Some(excerpt) = view.selection.clone() {
                                            view.insert_quote(&excerpt.text, window, cx);
                                        }
                                    })),
                            )
                            .child(
                                Button::new("selection-copy")
                                    .ghost()
                                    .small()
                                    .rounded_full_style(cx)
                                    .label(tr("message_copy"))
                                    .debug_selector(|| "message-selection-copy".into())
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        if let Some(excerpt) = &view.selection {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                excerpt.text.clone(),
                                            ));
                                        }
                                        view.dismiss_selection(cx);
                                    })),
                            )
                            .child(
                                Button::new("selection-new")
                                    .ghost()
                                    .small()
                                    .rounded_full_style(cx)
                                    .label(tr("chat_new"))
                                    .debug_selector(|| "message-selection-new".into())
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        if let Some(excerpt) = view.selection.take() {
                                            cx.emit(Event::NewConversation(excerpt.text));
                                        }
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .with_priority(gpui_kit::base::POPUP_PRIORITY)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests;

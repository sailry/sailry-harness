//! Preview controls only mutate the in-memory conversation fixture.
use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::{Textarea, TextareaState},
};

impl Shell {
    pub(super) fn user_footer(
        &self,
        location: Location,
        turn: &Turn,
        cx: &mut Context<Self>,
    ) -> MessageFooter {
        let id = location.id();
        MessageFooter::new().content_inset(false).w_full().child(
            h_flex()
                .id(format!("{id}-user-footer"))
                .w_full()
                .justify_end()
                .group(format!("{id}-user-footer"))
                .gap_2()
                .child(
                    div()
                        .id(format!("{id}-user-time"))
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .opacity(0.)
                        .group_hover(format!("{id}-user-footer"), |style| style.opacity(1.))
                        .child(tr("turn_preview_time")),
                )
                .child(
                    Clipboard::new(format!("{id}-user-copy"))
                        .value(turn.prompt.clone())
                        .tooltip(tr("message_copy")),
                )
                .when(location.child.is_none(), |row| {
                    row.child(
                        Button::new(format!("{id}-user-edit"))
                            .ghost()
                            .small()
                            .icon(Icon::default().path("icons/reicon/writing.svg"))
                            .tooltip(tr("message_edit"))
                            .accessibility_label(tr("message_edit"))
                            .disabled(
                                self.conversations[&location.session]
                                    .turns
                                    .iter()
                                    .any(|turn| turn.status.active()),
                            )
                            .debug_selector(move || location.selector("user-edit", None))
                            .on_click(cx.listener(move |shell, _, window, cx| {
                                shell.edit_preview(location, window, cx)
                            })),
                    )
                }),
        )
    }

    fn edit_preview(&self, location: Location, window: &mut Window, cx: &mut Context<Self>) {
        let Some(turn) = self.transcript_turn(location) else {
            return;
        };
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(tr("composer"))
                .auto_grow(2, 8)
                .default_value(turn.prompt.clone())
        });
        let owner = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let input = input.clone();
            let owner = owner.clone();
            dialog
                .form_title(tr("message_edit"))
                .child(Textarea::new(&input))
                .child(div().text_sm().child(tr("message_edit_notice")))
                .on_ok(move |_, window, cx| {
                    let text = input.read(cx).value();
                    if text.trim().is_empty() {
                        return false;
                    }
                    let _ = owner.update(cx, |shell, cx| {
                        let Some(thread) = shell.conversations.get_mut(&location.session) else {
                            return;
                        };
                        if thread.turns.iter().any(|turn| turn.status.active()) {
                            return;
                        }
                        let options = thread.turns[location.turn].options.clone();
                        thread.turns.truncate(location.turn);
                        let mut turn = Turn::new(text);
                        turn.options = options;
                        thread.turns.push(turn);
                        thread
                            .scroller
                            .update(cx, |state, cx| state.reset(thread.turns.len(), cx));
                        shell.animate_preview(location.session, window, cx);
                        cx.notify();
                    });
                    true
                })
        });
    }
}

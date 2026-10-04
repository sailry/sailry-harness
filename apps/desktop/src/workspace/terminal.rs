use super::{Command, Dispatch, Target};
use crate::{shell::Shell, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

impl Shell {
    pub(crate) fn terminal_overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let key = self
            .workspace
            .terminal
            .filter(|key| key.0 == self.host)
            .or_else(|| {
                self.workspace
                    .terminals
                    .iter()
                    .find(|(_, t)| t.owner == self.workspace.owner(self.host))
                    .map(|(&key, _)| key)
            });
        let Some((key, terminal)) =
            key.and_then(|key| self.workspace.terminals.get(&key).map(|t| (key, t)))
        else {
            let owner = self.workspace.owner(self.host);
            return v_flex()
                .size_full()
                .justify_center()
                .items_center()
                .gap_3()
                .child(tr("workspace_no_terminal"))
                .child(
                    Button::new("empty-new-terminal")
                        .primary()
                        .icon(IconName::Plus)
                        .label(tr("workspace_new_terminal"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.create_terminal(owner, window, cx)
                        })),
                )
                .into_any_element();
        };
        let worktree = &self.workspace.worktrees[&terminal.owner.worktree];
        v_flex()
            .id("terminal-overview")
            .debug_selector(|| "terminal-overview".into())
            .size_full()
            .p_6()
            .gap_4()
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(worktree.path.clone()),
                    )
                    .child(
                        Button::new("terminal-reset")
                            .ghost()
                            .icon(IconName::RotateCw)
                            .tooltip(tr("workspace_reset_terminal"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.workspace_action(
                                    &Dispatch {
                                        target: Target::Terminal(key),
                                        command: Command::Reset,
                                    },
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new("terminal-close")
                            .ghost()
                            .icon(IconName::Close)
                            .tooltip(tr("workspace_close_terminal"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.workspace_action(
                                    &Dispatch {
                                        target: Target::Terminal(key),
                                        command: Command::Close,
                                    },
                                    window,
                                    cx,
                                )
                            })),
                    ),
            )
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("workspace_terminal_preview")),
            )
            .when_some(terminal.profile.as_ref(), |this, profile| {
                this.child(
                    v_flex()
                        .gap_2()
                        .debug_selector(|| "terminal-cli-profile".into())
                        .child(div().truncate().font_semibold().child(profile.name.clone()))
                        .child(div().truncate().text_sm().child(profile.executable.clone()))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr("cli_preview")),
                        ),
                )
            })
            .child(
                div()
                    .debug_selector(|| "terminal-reset-count".into())
                    .text_sm()
                    .child(
                        rust_i18n::t!("workspace_reset_count", count = terminal.resets).to_string(),
                    ),
            )
            .into_any_element()
    }
}

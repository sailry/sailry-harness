use crate::{preview::Page, shell::Shell, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

impl Shell {
    pub(super) fn composer_context(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let host = self.host;
        let main = self.workspace.worktrees[&self.workspace.sessions[&(host, self.session)]
            .owner
            .worktree]
            .main;
        let owner = cx.entity().downgrade();
        h_flex()
            .debug_selector(|| "composer-context-bar".into())
            .relative()
            .mx_3()
            .px_2()
            .py_1()
            .gap_1()
            .flex_wrap()
            .text_color(cx.theme().muted_foreground)
            .child(
                Button::new("composer-host")
                    .rounded_full()
                    .ghost()
                    .icon(if host == 0 {
                        IconName::Cpu
                    } else {
                        IconName::Globe
                    })
                    .label(tr(if host == 0 {
                        "composer_host_local"
                    } else {
                        "composer_host_remote"
                    }))
                    .dropdown_caret(true)
                    .debug_selector(|| "composer-host".into())
                    .tooltip(
                        rust_i18n::t!(
                            "composer_host_hint",
                            name = tr(if host == 0 {
                                "local_host_name"
                            } else {
                                "remote_host_name"
                            })
                        )
                        .to_string(),
                    )
                    .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                        ["local_host_name", "remote_host_name"]
                            .into_iter()
                            .enumerate()
                            .fold(menu, |menu, (index, label)| {
                                let owner = owner.clone();
                                menu.item(
                                    PopupMenuItem::new(tr(label))
                                        .checked(host == index)
                                        .on_click(move |_, window, cx| {
                                            _ = owner.update(cx, |this, cx| {
                                                this.select_composer_host(index, window, cx)
                                            });
                                        }),
                                )
                            })
                    }),
            )
            .child(
                self.composer_choice(
                    1,
                    Button::new("composer-branch")
                        .rounded_full()
                        .max_w_56()
                        .debug_selector(|| "composer-branch".into())
                        .icon(if main {
                            IconName::Folder
                        } else {
                            IconName::Network
                        }),
                    &["composer_branch_main", "composer_branch_preview"],
                    cx,
                ),
            )
            .child(div().flex_1())
            .when(
                !self.conversations[&(host, self.session)].turns.is_empty(),
                |bar| bar.child(self.composer_usage(cx)),
            )
    }

    pub(crate) fn select_composer_host(
        &mut self,
        host: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.host == host {
            return;
        }
        // Keep the draft slot when changing its target, without moving data between hosts.
        let session = self.session;
        self.select_host(host, window, cx);
        self.session = session;
        self.navigate(Page::Conversation, window, cx);
    }
}

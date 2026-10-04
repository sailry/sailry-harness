use super::{Owner, model::Profile};
use crate::theme::DialogStyle as _;
use crate::{shell::Shell, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    scroll::ScrollableElement,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
pub(crate) mod menu;

struct Launcher {
    shell: WeakEntity<Shell>,
    owner: Owner,
    path: SharedString,
    profiles: Vec<Profile>,
    error: Option<&'static str>,
}

impl Shell {
    pub(crate) fn cli_launcher(&self, owner: Owner, window: &mut Window, cx: &mut Context<Self>) {
        if !self.workspace.contains(owner) {
            return;
        }
        if !self.workspace.projects[&owner.project].trusted {
            self.project_trust(
                owner.project,
                Some(super::Pending::LaunchCli(owner)),
                window,
                cx,
            );
            return;
        }
        let shell = cx.entity().downgrade();
        let path = self.workspace.worktrees[&owner.worktree].path.clone();
        let profiles = self
            .workspace
            .cli_profiles
            .get(&owner.host)
            .into_iter()
            .flatten()
            .filter(|profile| profile.available)
            .cloned()
            .collect();
        let launcher = cx.new(|_| Launcher {
            shell,
            owner,
            path,
            profiles,
            error: None,
        });
        launcher.update(cx, |_, cx| {
            crate::feedback::observe(window, cx, |view, _| view.error.into_iter().collect());
        });
        window.open_dialog(cx, move |dialog, window, _| {
            dialog
                .form_title(tr("cli_launch"))
                .w((window.viewport_size().width - px(48.)).min(px(480.)))
                .overlay_closable(false)
                .on_ok(|_, _, _| false)
                .child(launcher.clone())
        });
    }
}

impl Launcher {
    fn launch(&mut self, profile: &Profile, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.shell.update(cx, |shell, cx| {
            let key = shell
                .workspace
                .create_cli_terminal(self.owner, &self.path, profile)?;
            shell.sidebar.open_project(self.owner.project);
            shell.select_terminal(key, window, cx);
            Ok::<_, &'static str>(())
        });
        match result {
            Ok(Ok(())) => window.close_dialog(cx),
            Ok(Err(error)) => self.error = Some(error),
            Err(_) => self.error = Some("cli_target_changed"),
        }
        cx.notify();
    }
}

impl Render for Launcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .debug_selector(|| "cli-launcher".into())
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("cli_preview")),
            )
            .child(
                div()
                    .truncate()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .debug_selector(|| "cli-path".into())
                    .child(self.path.clone()),
            )
            .child(
                div()
                    .max_h((window.viewport_size().height - px(340.)).clamp(px(100.), px(300.)))
                    .child(
                        v_flex()
                            .id("cli-profiles")
                            .gap_1()
                            .p_1()
                            .overflow_y_scrollbar()
                            .children(self.profiles.iter().cloned().map(|profile| {
                                Button::new(profile.id)
                                    .custom(crate::theme::subtle_button(cx))
                                    .w_full()
                                    .accessibility_label(profile.name.clone())
                                    .child(
                                        h_flex()
                                            .w_full()
                                            .gap_3()
                                            .child(
                                                Icon::new(IconName::SquareTerminal)
                                                    .size_4()
                                                    .flex_shrink_0(),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_sm()
                                                    .child(format!(
                                                        "{} ({})",
                                                        profile.name, profile.executable
                                                    )),
                                            ),
                                    )
                                    .debug_selector(move || format!("cli-profile-{}", profile.id))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.launch(&profile, window, cx)
                                    }))
                            }))
                            .when(self.profiles.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_sm()
                                        .debug_selector(|| "cli-empty".into())
                                        .child(tr("cli_empty")),
                                )
                            }),
                    ),
            )
            .child(
                Button::new("cli-cancel")
                    .self_start()
                    .label(tr("settings_cancel"))
                    .debug_selector(|| "cli-cancel".into())
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
    }
}

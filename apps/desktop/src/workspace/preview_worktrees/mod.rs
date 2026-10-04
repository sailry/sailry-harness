use super::Owner;
use crate::theme::DialogStyle as _;
use crate::{preview::Page, shell::Shell, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    form::field,
    input::{Input, InputState},
    list::ListItem,
    scroll::ScrollableElement,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

mod removal;

impl Shell {
    pub(crate) fn worktree_manager(
        &self,
        project: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.workspace.project_owner(project) else {
            return;
        };
        let shell = cx.entity().downgrade();
        let create_reason = self.workspace.new_worktree_reason(project);
        let records: Vec<_> = self
            .workspace
            .worktrees
            .iter()
            .filter(|(_, tree)| tree.project == project)
            .map(|(&id, tree)| {
                (
                    id,
                    self.workspace.branch_label(Owner {
                        worktree: id,
                        ..target
                    }),
                    tree.path.clone(),
                    tree.base_ref.clone(),
                    self.workspace.removal_reason(Owner {
                        worktree: id,
                        ..target
                    }),
                )
            })
            .collect();
        window.open_dialog(cx, move |dialog, window, cx| {
            let rows = records
                .iter()
                .map(|(id, branch, path, base_ref, reason)| {
                    let id = *id;
                    let shell = shell.clone();
                    let remove = shell.clone();
                    let owner = Owner {
                        worktree: id,
                        ..target
                    };
                    ListItem::new(("worktree", id))
                        .debug_selector(move || format!("worktree-row-{id}"))
                        .rounded(cx.theme().radius)
                        .confirmed(target.worktree == id)
                        .child(
                            h_flex()
                                .min_w_0()
                                .gap_3()
                                .child(Icon::new(IconName::Network).size_4())
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap_0p5()
                                        .child(div().truncate().text_sm().child(branch.clone()))
                                        .child(
                                            div()
                                                .truncate()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(path.clone()),
                                        )
                                        .when_some(base_ref.clone(), |this, base| {
                                            this.child(
                                                div()
                                                    .truncate()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(format!(
                                                        "{} {base}",
                                                        tr("worktree_base")
                                                    )),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .w_5()
                                        .flex_shrink_0()
                                        .debug_selector(move || format!("worktree-check-{id}"))
                                        .when(target.worktree == id, |this| {
                                            this.child(Icon::new(IconName::Check).size_4())
                                        }),
                                )
                                .child(
                                    Button::new(("worktree-remove", id))
                                        .ghost()
                                        .small()
                                        .icon(IconName::CircleX)
                                        .flex_shrink_0()
                                        .disabled(reason.is_some())
                                        .tooltip(tr(reason.unwrap_or("worktree_remove")))
                                        .accessibility_label(
                                            tr(reason.unwrap_or("worktree_remove")),
                                        )
                                        .debug_selector(move || format!("worktree-remove-{id}"))
                                        .on_click(move |_, window, cx| {
                                            cx.stop_propagation();
                                            window.close_dialog(cx);
                                            _ = remove.update(cx, |shell, cx| {
                                                shell.remove_worktree_dialog(owner, window, cx)
                                            });
                                        }),
                                ),
                        )
                        .on_click(move |_, window, cx| {
                            _ = shell.update(cx, |this, cx| {
                                if this.workspace.contains(owner) {
                                    this.select_owner(owner, window, cx);
                                    this.navigate(Page::Project, window, cx);
                                }
                            });
                            window.close_dialog(cx);
                        })
                })
                .collect::<Vec<_>>();
            let shell = shell.clone();
            dialog
                .form_title(tr("worktree_manage"))
                .w((window.viewport_size().width - px(48.)).min(px(560.)))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("worktree_select_hint")),
                )
                .child(
                    div()
                        .debug_selector(|| "worktree-list".into())
                        .max_h(
                            (window.viewport_size().height - px(240.))
                                .max(px(120.))
                                .min(px(360.)),
                        )
                        .child(
                            v_flex()
                                .id("worktree-list")
                                .gap_1()
                                .overflow_y_scrollbar()
                                .children(rows),
                        ),
                )
                .footer(
                    gpui_kit::component::dialog::DialogFooter::new()
                        .w_full()
                        .child(
                            Button::new("worktree-new")
                                .primary()
                                .icon(IconName::Plus)
                                .label(tr("worktree_create"))
                                .disabled(create_reason.is_some())
                                .tooltip(tr(create_reason.unwrap_or("worktree_create")))
                                .debug_selector(|| "worktree-new".into())
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    _ = shell.update(cx, |this, cx| {
                                        this.new_worktree(target, window, cx)
                                    });
                                }),
                        ),
                )
        });
    }

    pub(crate) fn new_worktree(&self, target: Owner, window: &mut Window, cx: &mut Context<Self>) {
        if !self.workspace.contains(target) {
            return;
        }
        if let Some(reason) = self.workspace.new_worktree_reason(target.project) {
            crate::feedback::error(&tr("worktree_create"), &tr(reason), window, cx);
            return;
        }
        let base = self.workspace.worktrees[&target.worktree].branch.clone();
        let shell = cx.entity().downgrade();
        let form = cx.new(|cx| WorktreeForm {
            shell,
            project: target.project,
            inputs: [base, "".into(), "".into()]
                .into_iter()
                .zip([
                    "form_revision_hint",
                    "form_branch_hint",
                    "form_target_directory_hint",
                ])
                .map(|(value, hint)| {
                    cx.new(|cx| {
                        InputState::new(window, cx)
                            .default_value(value)
                            .placeholder(tr(hint))
                    })
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            error: None,
        });
        form.update(cx, |_, cx| {
            crate::feedback::observe(window, cx, |view, _| view.error.into_iter().collect());
        });
        window.open_dialog(cx, move |dialog, window, _| {
            let submit = form.clone();
            let confirm = form.clone();
            dialog
                .form_title(tr("worktree_create"))
                .w((window.viewport_size().width - px(48.)).min(px(480.)))
                .overlay_closable(false)
                .on_ok(move |_, window, cx| {
                    confirm.update(cx, |form, cx| form.submit(window, cx));
                    false
                })
                .child(form.clone())
                .footer(
                    gpui_kit::component::dialog::DialogFooter::new()
                        .w_full()
                        .gap_2()
                        .child(
                            Button::new("worktree-cancel")
                                .label(tr("settings_cancel"))
                                .debug_selector(|| "worktree-cancel".into())
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("worktree-submit")
                                .primary()
                                .label(tr("worktree_create_preview"))
                                .debug_selector(|| "worktree-submit".into())
                                .on_click(move |_, window, cx| {
                                    submit.update(cx, |form, cx| form.submit(window, cx))
                                }),
                        ),
                )
        });
    }
}

struct WorktreeForm {
    shell: WeakEntity<Shell>,
    project: usize,
    inputs: [Entity<InputState>; 3],
    error: Option<&'static str>,
}

impl WorktreeForm {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let [base, branch, path] = self.inputs.each_ref().map(|input| input.read(cx).value());
        let result = self
            .shell
            .update(cx, |shell, cx| {
                let owner = shell
                    .workspace
                    .add_worktree(self.project, &base, &branch, &path)?;
                shell.select_owner(owner, window, cx);
                shell.sidebar.open_project(owner.project);
                shell.navigate(Page::Project, window, cx);
                Ok::<_, &'static str>(())
            })
            .unwrap_or(Err("worktree_missing_project"));
        match result {
            Ok(()) => window.close_dialog(cx),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }
}

impl Render for WorktreeForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("worktree-form")
            .debug_selector(|| "worktree-form".into())
            .gap_4()
            .max_h((window.viewport_size().height - px(200.)).max(px(120.)))
            .overflow_y_scrollbar()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("worktree_preview_hint")),
            )
            .child(
                gpui_kit::component::form::Form::vertical().children(
                    ["worktree_base", "worktree_branch", "worktree_path"]
                        .into_iter()
                        .zip(&self.inputs)
                        .map(|(label, input)| {
                            field().label(tr(label)).required(true).child(
                                div()
                                    .debug_selector(move || format!("{label}-input"))
                                    .child(Input::new(input).aria_label(tr(label))),
                            )
                        }),
                ),
            )
    }
}

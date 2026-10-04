use crate::{shell::Shell, tr, workspace::Repository};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    scroll::ScrollableElement,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

mod records;

impl Shell {
    pub(crate) fn project_overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let owner = self.workspace.owner(self.host);
        let project = &self.workspace.projects[&owner.project];
        let branch = self.workspace.branch_label(owner);
        let directory = project.repository == Repository::Directory;
        v_flex()
            .id("project-overview")
            .debug_selector(|| "project-overview".into())
            .size_full()
            .overflow_y_scrollbar()
            .p_6()
            .child(
                v_flex().items_center().py_4().child(
                    v_flex()
                        .w_full()
                        .max_w(px(800.))
                        .items_start()
                        .gap_4()
                        .child(
                            h_flex()
                                .w_full()
                                .gap_3()
                                .child(
                                    div()
                                        .debug_selector(|| "project-overview-name".into())
                                        .min_w_0()
                                        .truncate()
                                        .text_xl()
                                        .font_semibold()
                                        .child(project.name.clone()),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(tr(if owner.host == 0 {
                                            "local_host_name"
                                        } else {
                                            "remote_host_name"
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .w_full()
                                .truncate()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(project.path.clone()),
                        )
                        .child(
                            h_flex()
                                .gap_4()
                                .flex_wrap()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .when(directory, |row| {
                                    row.child(
                                        Button::new("project-initialize")
                                            .ghost()
                                            .small()
                                            .icon(IconName::Network)
                                            .label(tr("git_initialize"))
                                            .debug_selector(|| "project-initialize".into())
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.initialize_git(owner.project, window, cx)
                                            })),
                                    )
                                })
                                .when(!directory, |row| {
                                    row.child(
                                        h_flex()
                                            .gap_2()
                                            .child(Icon::new(IconName::Network).size_4())
                                            .child(branch.clone()),
                                    )
                                }),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .flex_wrap()
                                .child(
                                    Button::new("project-new-session")
                                        .primary()
                                        .debug_selector(|| "project-new-session".into())
                                        .icon(IconName::Plus)
                                        .label(tr("workspace_new_session"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.create_session(owner, window, cx)
                                        })),
                                )
                                .child(
                                    Button::new("project-new-terminal")
                                        .debug_selector(|| "project-new-terminal".into())
                                        .icon(IconName::SquareTerminal)
                                        .label(tr("workspace_new_terminal"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.create_terminal(owner, window, cx)
                                        })),
                                )
                                .child(
                                    self.preview_cli_button(
                                        owner,
                                        Button::new("project-cli")
                                            .debug_selector(|| "project-cli".into())
                                            .icon(IconName::SquareTerminal)
                                            .label(tr("cli_launch")),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr("workspace_preview")),
                        )
                        .child(self.project_records(cx)),
                ),
            )
            .into_any_element()
    }
}

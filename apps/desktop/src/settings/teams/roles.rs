//! Roles tab: the shared Node role catalog, edited with exact revisions. This is the
//! native port of the delegation plugin's role page; the turn budget keeps its stored value.
use super::{
    Binding, Catalog,
    form::{self, Layout, RoleForm},
    list,
    requests::{self, Subject},
};
use crate::{
    settings::{Workspace, entry::Entry, group::Group},
    theme::DialogStyle as _,
    tr,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    notification::Notification,
    *,
};
use gpui_kit::*;
use sailry_protocol::{Command, Output, Request, RoleId, role};

impl Workspace {
    pub(super) fn role_rows(
        &self,
        catalog: Option<&Catalog>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let roles = catalog.map_or(&[][..], |catalog| catalog.roles.as_slice());
        let disabled = catalog.is_none();
        let rows = roles.iter().enumerate().map(|(index, role)| {
            let edit = role.clone();
            let remove = role.clone();
            let (name, description) = super::draft::label(role);
            Entry::new(
                format!("role-{index}"),
                h_flex()
                    .min_w_0()
                    .gap_3()
                    .child(list::role_tile(role.appearance.as_ref(), cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(div().truncate().child(name))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(if description.is_empty() {
                                        format!("@{}", role.key).into()
                                    } else {
                                        description
                                    }),
                            ),
                    ),
            )
            .action(
                Button::new(("role-edit", index))
                    .icon(IconName::Settings2)
                    .tooltip(tr("settings_edit"))
                    .accessibility_label(tr("settings_edit"))
                    .disabled(disabled)
                    .on_click(cx.listener(move |workspace, _, window, cx| {
                        workspace.open_role(Some(edit.clone()), window, cx)
                    })),
            )
            .action(
                Button::new(("role-delete", index))
                    .icon(gpui_kit::assets::IconName::Trash)
                    .tooltip(tr("settings_delete"))
                    .accessibility_label(tr("settings_delete"))
                    .disabled(disabled)
                    .on_click(cx.listener(move |workspace, _, window, cx| {
                        workspace.remove_role(remove.clone(), window, cx)
                    })),
            )
        });
        Group::new("teams_roles")
            .heading(false)
            .empty(IconName::Bot, "teams_roles_empty")
            .children(rows.collect::<Vec<_>>())
            .into_any_element()
    }

    pub(super) fn open_role(
        &mut self,
        profile: Option<role::Profile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(catalog) = self.team_catalog() {
            Dialog::open(catalog, profile, window, cx);
        }
    }

    fn remove_role(&mut self, role: role::Profile, window: &mut Window, cx: &mut Context<Self>) {
        let Some(catalog) = self.team_catalog() else {
            return;
        };
        let name = super::draft::label(&role).0;
        let prompt = rust_i18n::t!("teams_role_delete_prompt", name = name.as_ref()).to_string();
        crate::prompts::confirm(
            &tr("settings_delete"),
            &prompt,
            tr("settings_delete"),
            window,
            cx,
            move |window, cx| {
                requests::remove(
                    catalog.binding,
                    Command::RemoveRole {
                        role: role.id,
                        expected_revision: role.revision,
                    },
                    Subject::Role,
                    "teams_role_deleted",
                    window,
                    cx,
                    |_, _| {},
                )
            },
        );
    }
}

/// One role dialog; an uncertain save keeps its request for an explicit retry.
struct Dialog {
    binding: Binding,
    form: Entity<RoleForm>,
    request: Option<Request>,
    pending: bool,
    error: Option<&'static str>,
}

impl Dialog {
    fn open(catalog: Catalog, profile: Option<role::Profile>, window: &mut Window, cx: &mut App) {
        let creating = profile.is_none();
        let owner = cx.new(|cx| {
            crate::feedback::observe(window, cx, |dialog: &Dialog, _| {
                dialog.error.into_iter().collect()
            });
            let original = profile.unwrap_or_else(|| form::blank(RoleId::new()));
            let models = catalog.models.clone();
            let form =
                cx.new(|cx| RoleForm::new(original, creating, models, Layout::Dialog, window, cx));
            cx.observe(&form, |_, _, cx| cx.notify()).detach();
            Dialog {
                binding: catalog.binding.clone(),
                form,
                request: None,
                pending: false,
                error: None,
            }
        });
        window.open_dialog(cx, move |dialog, window, cx| {
            dialog
                .form_title(
                    tr(if creating {
                        "role_new"
                    } else {
                        "teams_role_edit"
                    }),
                    cx,
                )
                .w((window.viewport_size().width - px(48.)).min(px(560.)))
                .overlay_closable(false)
                .child(owner.clone())
        });
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        if self.request.is_none() {
            let form = self.form.read(cx);
            match form.profile(None, cx) {
                Ok(role) => {
                    let expected_revision = form.original.revision;
                    self.request = Some(self.binding.client.prepare(Command::PutRole {
                        role,
                        expected_revision,
                    }));
                }
                Err(key) => {
                    self.error = Some(key);
                    cx.notify();
                    return;
                }
            }
        }
        let Some(request) = self.request.clone() else {
            return;
        };
        self.pending = true;
        self.error = None;
        self.form.update(cx, |form, cx| form.lock(true, cx));
        let job = requests::execute(&self.binding, request);
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |dialog, window, cx| {
                dialog.pending = false;
                match result {
                    Ok(Ok(Output::Role(_))) => {
                        dialog.request = None;
                        crate::feedback::toast(
                            window,
                            tr("teams_role_saved"),
                            Notification::success(tr("teams_role_saved")),
                            cx,
                        );
                        window.close_dialog(cx);
                    }
                    Ok(Err(fault)) => {
                        if !requests::uncertain(fault.code) {
                            dialog.request = None;
                        }
                        dialog.error = Some(requests::key(Subject::Role, fault.code));
                    }
                    _ => dialog.error = Some("role_unknown"),
                }
                let locked = dialog.request.is_some();
                dialog.form.update(cx, |form, cx| form.lock(locked, cx));
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

impl Render for Dialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let retry = self.request.is_some() && !self.pending;
        v_flex().w_full().gap_6().child(self.form.clone()).child(
            h_flex()
                .w_full()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("role-cancel")
                        .label(tr("settings_cancel"))
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .child(
                    Button::new("role-save")
                        .primary()
                        .label(tr(if retry { "role_retry" } else { "settings_save" }))
                        .disabled(self.pending)
                        .on_click(cx.listener(|dialog, _, window, cx| dialog.save(window, cx))),
                ),
        )
    }
}

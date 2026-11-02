//! The team modal: one Kit dialog with the member list on the left and the selected
//! member on the right. Role changes and the team are written together on Save.
mod members;
mod save;

use super::{
    Catalog,
    form::{self, Layout, RoleForm},
    models::ModelField,
};
use crate::{theme::DialogStyle as _, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    form::Field,
    input::{Input, InputState, Textarea, TextareaState},
    scroll::ScrollableElement as _,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sailry_protocol::{Request, RoleId, TeamId, projects::Appearance, role, team};
use std::collections::BTreeSet;

pub(super) enum Target {
    New,
    Edit(team::Profile),
    /// A new team prefilled from a stored team, or from the Default team when None.
    Copy(Option<team::Profile>),
    /// The virtual Default team, shown read-only.
    Default,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Selected {
    Lead,
    Member(RoleId),
}

struct Member {
    id: RoleId,
    /// The catalog role; None until a role created here is first saved.
    original: Option<role::Profile>,
    /// Created when the member is first selected; untouched members are not rewritten.
    form: Option<Entity<RoleForm>>,
}

/// One revision-checked write in a team save.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Role(RoleId),
    Team,
}

pub(super) struct Editor {
    catalog: Catalog,
    template: team::Profile,
    original: Option<team::Profile>,
    builtin: bool,
    name: Entity<InputState>,
    description: Entity<InputState>,
    appearance: Option<Appearance>,
    lead: Entity<ModelField>,
    instructions: Entity<TextareaState>,
    members: Vec<Member>,
    /// Members present when the editor opened; "Add role" lists the others.
    baseline: BTreeSet<RoleId>,
    selected: Selected,
    adder: Entity<members::Adder>,
    /// The write in flight or with an uncertain outcome, retried with the same request ID.
    request: Option<(Step, Request)>,
    /// Roles written during the current save.
    saved: BTreeSet<RoleId>,
    pending: bool,
    error: Option<&'static str>,
    _subscriptions: Vec<Subscription>,
}

/// The virtual Default team: every catalog role, following the conversation model.
fn virtual_team(catalog: &Catalog) -> team::Profile {
    team::Profile {
        id: TeamId::new(),
        revision: 0,
        key: String::new(),
        name: tr("team_default").to_string(),
        appearance: None,
        description: tr("team_default_description").to_string(),
        model: None,
        instructions: String::new(),
        members: catalog.roles.iter().map(|role| role.id).collect(),
    }
}

pub(super) fn open(catalog: Catalog, target: Target, window: &mut Window, cx: &mut App) {
    let editor = cx.new(|cx| Editor::new(catalog, target, window, cx));
    window.open_dialog(cx, move |dialog, window, cx| {
        // Same padding as the other settings forms, so Kit places the close button alike.
        dialog
            .floating_surface(cx)
            .p_6()
            .w((window.viewport_size().width - px(48.)).min(px(860.)))
            .overlay_closable(false)
            .child(editor.clone())
    });
}

impl Editor {
    fn new(catalog: Catalog, target: Target, window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::feedback::observe(window, cx, |editor: &Editor, _| {
            editor.error.into_iter().collect()
        });
        let (template, original, builtin) = match target {
            Target::New => (
                team::Profile {
                    id: TeamId::new(),
                    revision: 0,
                    key: String::new(),
                    name: String::new(),
                    appearance: Some(Appearance::random()),
                    description: String::new(),
                    model: None,
                    instructions: String::new(),
                    members: Vec::new(),
                },
                None,
                false,
            ),
            Target::Edit(team) => (team.clone(), Some(team), false),
            Target::Copy(source) => {
                let mut copy = source.unwrap_or_else(|| team::Profile {
                    description: String::new(),
                    ..virtual_team(&catalog)
                });
                copy.name = rust_i18n::t!("team_copy_name", name = copy.name.as_str()).to_string();
                copy.id = TeamId::new();
                copy.revision = 0;
                copy.key = String::new();
                copy.appearance.get_or_insert_with(Appearance::random);
                (copy, None, false)
            }
            Target::Default => (virtual_team(&catalog), None, true),
        };
        let members: Vec<_> = template
            .members
            .iter()
            .filter_map(|id| catalog.roles.iter().find(|role| role.id == *id))
            .map(|role| Member {
                id: role.id,
                original: Some(role.clone()),
                form: None,
            })
            .collect();
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(template.name.clone())
                .placeholder(tr("team_name"))
        });
        let description = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(template.description.clone())
                .placeholder(tr("teams_description"))
        });
        let models = catalog.models.clone();
        let model = template.model.clone();
        let lead = cx.new(|cx| ModelField::new(models, model, "team_follow_conversation", cx));
        let instructions = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(6, 14)
                .default_value(template.instructions.clone())
        });
        let owner = cx.weak_entity();
        let adder = cx.new(|_| members::Adder::new(owner));
        let subscriptions = vec![cx.observe(&lead, |_, _, cx| cx.notify())];
        let mut editor = Self {
            baseline: members.iter().map(|member| member.id).collect(),
            catalog,
            appearance: template.appearance.clone(),
            template,
            original,
            builtin,
            name,
            description,
            lead,
            instructions,
            members,
            selected: Selected::Lead,
            adder,
            request: None,
            saved: BTreeSet::new(),
            pending: false,
            error: None,
            _subscriptions: subscriptions,
        };
        editor.sync(cx);
        editor
    }

    /// Default-team views, writes in flight and uncertain writes keep the draft read-only.
    fn locked(&self) -> bool {
        self.builtin || self.pending || self.request.is_some()
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        let locked = self.locked();
        self.lead
            .update(cx, |lead, cx| lead.set_disabled(locked, cx));
        for form in self.members.iter().filter_map(|member| member.form.clone()) {
            form.update(cx, |form, cx| form.lock(locked, cx));
        }
        cx.notify();
    }

    fn member(&self, id: RoleId) -> Option<&Member> {
        self.members.iter().find(|member| member.id == id)
    }

    fn mount(
        &mut self,
        original: role::Profile,
        creating: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<RoleForm> {
        let models = self.catalog.models.clone();
        let locked = self.locked();
        let form = cx.new(|cx| {
            let mut form = RoleForm::new(original, creating, models, Layout::Member, window, cx);
            form.lock(locked, cx);
            form
        });
        self._subscriptions
            .push(cx.observe(&form, |_, _, cx| cx.notify()));
        form
    }

    fn select(&mut self, selected: Selected, window: &mut Window, cx: &mut Context<Self>) {
        if let Selected::Member(id) = selected {
            let Some(index) = self.members.iter().position(|member| member.id == id) else {
                return;
            };
            if self.members[index].form.is_none() {
                let original = self.members[index]
                    .original
                    .clone()
                    .unwrap_or_else(|| form::blank(id));
                let form = self.mount(original, false, window, cx);
                self.members[index].form = Some(form);
            }
        }
        self.selected = selected;
        cx.notify();
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let locked = self.locked();
        h_flex()
            .w_full()
            .gap_3()
            .pr_8()
            .child(crate::workspace::appearance::picker(
                "team",
                cx.entity().downgrade(),
                crate::workspace::appearance::Selection {
                    selected: self
                        .appearance
                        .clone()
                        .unwrap_or_else(super::draft::fallback),
                    read: |editor: &Editor| {
                        editor
                            .appearance
                            .clone()
                            .unwrap_or_else(super::draft::fallback)
                    },
                    write: |editor, value| editor.appearance = Some(value),
                },
                crate::workspace::appearance::Trigger::Avatar,
                locked,
                tr("role_appearance"),
                cx,
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        Input::new(&self.name)
                            .appearance(false)
                            .disabled(locked)
                            .aria_label(tr("team_name")),
                    )
                    .child(
                        Input::new(&self.description)
                            .appearance(false)
                            .small()
                            .disabled(locked)
                            .aria_label(tr("teams_description")),
                    ),
            )
    }

    fn details(&self) -> AnyElement {
        match self.selected {
            Selected::Lead => v_flex()
                .w_full()
                .gap_4()
                .child(div().font_semibold().child(tr("team_lead")))
                .child(
                    Field::new()
                        .label(tr("teams_model"))
                        .child(self.lead.clone()),
                )
                .child(
                    Field::new().label(tr("role_instructions")).child(
                        Textarea::new(&self.instructions)
                            .readonly(self.locked())
                            .aria_label(tr("role_instructions")),
                    ),
                )
                .into_any_element(),
            Selected::Member(id) => self
                .member(id)
                .and_then(|member| member.form.clone())
                .map_or_else(|| div().into_any_element(), IntoElement::into_any_element),
        }
    }

    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let retry = self.request.is_some() && !self.pending;
        let removable = self.original.is_some() && !self.builtin;
        h_flex()
            .w_full()
            .gap_2()
            .pt_4()
            .border_t_1()
            .border_color(cx.theme().border)
            .when(removable, |footer| {
                footer.child(
                    Button::new("team-delete")
                        .danger()
                        .label(tr("team_delete"))
                        .disabled(self.locked())
                        .on_click(cx.listener(|editor, _, window, cx| editor.delete(window, cx))),
                )
            })
            .child(div().flex_1())
            .child(
                Button::new("team-cancel")
                    .label(tr(if self.builtin {
                        "close"
                    } else {
                        "settings_cancel"
                    }))
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .when(!self.builtin, |footer| {
                footer.child(
                    Button::new("team-save")
                        .primary()
                        .label(tr(if retry { "role_retry" } else { "settings_save" }))
                        .disabled(self.pending)
                        .on_click(cx.listener(|editor, _, window, cx| editor.save(window, cx))),
                )
            })
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(team) = self.original.clone().filter(|_| !self.locked()) else {
            return;
        };
        super::menu::delete(
            self.catalog.binding.clone(),
            team,
            window,
            cx,
            |window, cx| window.close_dialog(cx),
        );
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let height = (window.viewport_size().height - px(280.))
            .max(px(260.))
            .min(px(480.));
        v_flex()
            .w_full()
            .gap_4()
            .child(self.header(cx))
            .child(
                h_flex()
                    .w_full()
                    .h(height)
                    .items_start()
                    .gap_4()
                    .child(self.column(cx))
                    .child(
                        v_flex()
                            .id("team-details")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scrollbar()
                            .child(self.details()),
                    ),
            )
            .child(self.footer(cx))
    }
}

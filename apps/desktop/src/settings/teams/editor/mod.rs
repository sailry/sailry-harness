//! The team dialog: one form column with the team identity, the lead's model and prompt,
//! then the member list. Teams only reference catalog roles; roles are edited in the
//! Roles tab, so Save writes the team alone.
mod members;
mod save;

use super::{Catalog, models::ModelField};
use crate::{theme::DialogStyle as _, tr, workspace::appearance as identity};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    form::Field,
    input::{Input, InputState, Textarea, TextareaState},
    scroll::ScrollableElement as _,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sailry_protocol::{Request, RoleId, TeamId, projects::Appearance, team};
use std::collections::BTreeSet;

pub(super) enum Target {
    New,
    Edit(team::Profile),
    /// A new team prefilled from a stored team, or from the Default team when None.
    Copy(Option<team::Profile>),
    /// The virtual Default team, shown read-only.
    Default,
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
    members: Vec<RoleId>,
    /// Members present when the editor opened; "Add role" lists the others.
    baseline: BTreeSet<RoleId>,
    adder: Entity<members::Adder>,
    /// The write in flight or with an uncertain outcome, retried with the same request ID.
    request: Option<Request>,
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
    let title = tr(match target {
        Target::Edit(_) => "team_edit",
        Target::Default => "team_default",
        Target::New | Target::Copy(_) => "team_new",
    });
    let editor = cx.new(|cx| Editor::new(catalog, target, window, cx));
    window.open_dialog(cx, move |dialog, window, cx| {
        dialog
            .form_title(title.clone(), cx)
            .w((window.viewport_size().width - px(48.)).min(px(520.)))
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
        // Members whose role left the catalog are dropped from the draft.
        let members: Vec<_> = template
            .members
            .iter()
            .copied()
            .filter(|id| catalog.roles.iter().any(|role| role.id == *id))
            .collect();
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(template.name.clone())
                .placeholder(tr("form_name_hint"))
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
                .auto_grow(3, 10)
                .default_value(template.instructions.clone())
        });
        let owner = cx.weak_entity();
        let adder = cx.new(|_| members::Adder::new(owner));
        let subscriptions = vec![cx.observe(&lead, |_, _, cx| cx.notify())];
        let mut editor = Self {
            baseline: members.iter().copied().collect(),
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
            adder,
            request: None,
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
        cx.notify();
    }

    fn fields(&self, cx: &mut Context<Self>) -> [AnyElement; 4] {
        let locked = self.locked();
        let picker = identity::picker(
            "team",
            cx.entity().downgrade(),
            identity::Selection {
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
            locked,
            tr("role_appearance"),
            cx,
        );
        [
            Field::new()
                .label(tr("settings_name"))
                .child(
                    Input::new(&self.name)
                        .prefix(picker)
                        .disabled(locked)
                        .aria_label(tr("settings_name")),
                )
                .into_any_element(),
            Field::new()
                .label(tr("teams_description"))
                .child(
                    Input::new(&self.description)
                        .disabled(locked)
                        .aria_label(tr("teams_description")),
                )
                .into_any_element(),
            Field::new()
                .label(tr("team_lead_model"))
                .child(self.lead.clone())
                .into_any_element(),
            Field::new()
                .label(tr("team_lead_instructions"))
                .child(
                    Textarea::new(&self.instructions)
                        .readonly(locked)
                        .aria_label(tr("team_lead_instructions")),
                )
                .into_any_element(),
        ]
    }

    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let footer = h_flex()
            .w_full()
            .gap_2()
            .pt_4()
            .border_t_1()
            .border_color(cx.theme().border);
        if self.builtin {
            return footer
                .child(div().flex_1())
                .child(
                    Button::new("team-copy")
                        .label(tr("team_duplicate"))
                        .on_click(cx.listener(|editor, _, window, cx| editor.copy(window, cx))),
                )
                .child(
                    Button::new("team-close")
                        .label(tr("close"))
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                );
        }
        let retry = self.request.is_some() && !self.pending;
        let save = if retry {
            "role_retry"
        } else if self.original.is_some() {
            "settings_save"
        } else {
            "team_create"
        };
        footer
            .when(self.original.is_some(), |footer| {
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
                    .label(tr("settings_cancel"))
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                Button::new("team-save")
                    .primary()
                    .label(tr(save))
                    .disabled(self.pending)
                    .on_click(cx.listener(|editor, _, window, cx| editor.save(window, cx))),
            )
    }

    /// Replaces the read-only Default team with an editable copy.
    fn copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let catalog = self.catalog.clone();
        window.close_dialog(cx);
        open(catalog, Target::Copy(None), window, cx);
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
        let height = (window.viewport_size().height - px(240.)).max(px(260.));
        let fields = (!self.builtin).then(|| self.fields(cx));
        v_flex()
            .w_full()
            .gap_6()
            .child(
                v_flex()
                    .id("team-form")
                    .w_full()
                    .max_h(height)
                    .gap_4()
                    .overflow_y_scrollbar()
                    .children(fields.into_iter().flatten())
                    .child(self.member_list(cx)),
            )
            .child(self.footer(cx))
    }
}

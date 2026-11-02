//! One role form serves the Roles tab dialog and the team editor's member column.
//! Fields without a control (turn budget, skills, tools) keep their stored values.
use super::{
    draft::{self, Preset},
    models::{ModelField, Models},
};
use crate::{tr, workspace::appearance as identity};
use gpui_kit::component::{
    button::Button,
    form::Field,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
    popover::Popover,
    switch::Switch,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sailry_protocol::{RoleId, projects::Appearance, role};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Layout {
    /// Roles tab: presets and the selection identifier are editable.
    Dialog,
    /// Team editor: identity inline above model, instructions and read-only.
    Member,
}

pub(super) fn blank(id: RoleId) -> role::Profile {
    role::Profile {
        id,
        revision: 0,
        key: String::new(),
        name: String::new(),
        appearance: Some(Appearance::random()),
        description: String::new(),
        read_only: false,
        model: None,
        max_turns: None,
        skills: Vec::new(),
        tools: None,
        instructions: String::new(),
    }
}

pub(super) struct RoleForm {
    pub original: role::Profile,
    pub creating: bool,
    layout: Layout,
    name: Entity<InputState>,
    key: Entity<InputState>,
    description: Entity<InputState>,
    instructions: Entity<TextareaState>,
    appearance: Option<Appearance>,
    /// Localized seeded text shown in place of the stored name and description.
    shown: (SharedString, SharedString),
    read_only: bool,
    model: Entity<ModelField>,
    locked: bool,
    _subscriptions: Vec<Subscription>,
}

impl RoleForm {
    pub fn new(
        original: role::Profile,
        creating: bool,
        models: Models,
        layout: Layout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let shown = draft::label(&original);
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(shown.0.clone())
                .placeholder(tr("form_name_hint"))
        });
        let key = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(original.key.clone())
                .placeholder(tr("teams_key_hint"))
        });
        let description = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(shown.1.clone())
                .placeholder(tr("teams_description"))
        });
        let instructions = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(5, 12)
                .default_value(original.instructions.clone())
        });
        let model =
            cx.new(|cx| ModelField::new(models, original.model.clone(), "teams_follow_lead", cx));
        let mut subscriptions = vec![cx.observe(&model, |_, _, cx| cx.notify())];
        for input in [&name, &description] {
            subscriptions.push(cx.subscribe(input, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }));
        }
        Self {
            appearance: original.appearance.clone(),
            shown,
            read_only: original.read_only,
            original,
            creating,
            layout,
            name,
            key,
            description,
            instructions,
            model,
            locked: false,
            _subscriptions: subscriptions,
        }
    }

    pub fn name(&self, cx: &App) -> SharedString {
        self.name.read(cx).value().trim().to_owned().into()
    }

    pub fn appearance(&self) -> Option<&Appearance> {
        self.appearance.as_ref()
    }

    pub fn model(&self, cx: &App) -> Option<role::Model> {
        self.model.read(cx).value().cloned()
    }

    pub fn read_only(&self) -> bool {
        self.read_only
    }

    pub fn lock(&mut self, locked: bool, cx: &mut Context<Self>) {
        self.locked = locked;
        self.model
            .update(cx, |model, cx| model.set_disabled(locked, cx));
        cx.notify();
    }

    /// Adopts the Node's saved revision so a later save sends it as the expected revision.
    pub fn accept(&mut self, saved: role::Profile) {
        self.original = saved;
        self.creating = false;
    }

    /// Builds the profile to save. `key` replaces the identifier field's value when given.
    pub fn profile(&self, key: Option<String>, cx: &App) -> Result<role::Profile, &'static str> {
        let key = key.unwrap_or_else(|| self.key.read(cx).value().trim().to_owned());
        let mut name = self.name(cx).to_string();
        if !draft::valid_key(&key) || name.is_empty() {
            return Err("teams_role_invalid");
        }
        // Unedited localized seeded text keeps the stored text, so it stays localizable.
        if name == self.shown.0.as_ref() {
            name.clone_from(&self.original.name);
        }
        let mut description = self.description.read(cx).value().to_string();
        if description == self.shown.1.as_ref() {
            description.clone_from(&self.original.description);
        }
        Ok(role::Profile {
            key,
            name,
            appearance: self.appearance.clone(),
            description,
            read_only: self.read_only,
            model: self.model(cx),
            instructions: self.instructions.read(cx).value().to_string(),
            ..self.original.clone()
        })
    }

    fn apply(&mut self, preset: Preset, window: &mut Window, cx: &mut Context<Self>) {
        if self.locked {
            return;
        }
        if self.creating {
            self.original.max_turns = preset.max_turns();
            self.original.skills = preset.skills();
            self.appearance = Some(preset.appearance());
            self.read_only = preset.read_only();
            self.model.update(cx, |model, cx| model.set(None, cx));
            for (input, value) in [
                (&self.key, SharedString::from(preset.key())),
                (&self.name, tr(preset.label())),
                (&self.description, tr(preset.description())),
            ] {
                input.update(cx, |input, cx| input.set_value(value, window, cx));
            }
        }
        let instructions = tr(preset.instructions());
        self.instructions
            .update(cx, |input, cx| input.set_value(instructions, window, cx));
        cx.notify();
    }

    /// `field` matches the form's inputs; otherwise it sits small beside the prompt label.
    fn presets(&self, field: bool, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        Button::new("role-presets")
            .outline()
            .when(field, |button| button.w_full())
            .when(!field, |button| button.small())
            .label(tr("role_presets"))
            .dropdown_caret(true)
            .disabled(self.locked)
            .dropdown_menu(move |menu, _, _| {
                Preset::ALL.into_iter().fold(menu, |menu, preset| {
                    let owner = owner.clone();
                    menu.item(PopupMenuItem::new(tr(preset.label())).on_click(
                        move |_, window, cx| {
                            _ = owner.update(cx, |form, cx| form.apply(preset, window, cx));
                        },
                    ))
                })
            })
            .into_any_element()
    }

    fn identity(&self, trigger: identity::Trigger, cx: &mut Context<Self>) -> Popover {
        identity::picker(
            "role",
            cx.entity().downgrade(),
            identity::Selection {
                selected: self.appearance.clone().unwrap_or_else(draft::fallback),
                read: |form: &RoleForm| form.appearance.clone().unwrap_or_else(draft::fallback),
                write: |form, value| form.appearance = Some(value),
            },
            trigger,
            self.locked,
            tr("role_appearance"),
            cx,
        )
    }

    fn read_only_row(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        h_flex()
            .w_full()
            .gap_4()
            .text_sm()
            .child(div().flex_1().min_w_0().child(tr("teams_read_only")))
            .child(
                Switch::new("role-read-only")
                    .checked(self.read_only)
                    .disabled(self.locked)
                    .accessibility_label(tr("teams_read_only"))
                    .on_click(cx.listener(|form, value: &bool, _, cx| {
                        if !form.locked {
                            form.read_only = *value;
                            cx.notify();
                        }
                    })),
            )
    }

    fn prompt(&self, presets: Option<AnyElement>) -> impl IntoElement + use<> {
        v_flex()
            .w_full()
            .gap_2()
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .gap_2()
                    .text_sm()
                    .font_medium()
                    .child(tr("role_instructions"))
                    .children(presets),
            )
            .child(
                Textarea::new(&self.instructions)
                    .readonly(self.locked)
                    .aria_label(tr("role_instructions")),
            )
    }

    fn dialog(&self, cx: &mut Context<Self>) -> AnyElement {
        let locked = self.locked;
        let pair = |left: Field, right: Field| {
            h_flex()
                .w_full()
                .gap_4()
                .items_start()
                .child(div().flex_1().min_w_0().child(left))
                .child(div().flex_1().min_w_0().child(right))
        };
        v_flex()
            .w_full()
            .gap_4()
            .children(self.creating.then(|| {
                Field::new()
                    .label(tr("role_presets"))
                    .child(self.presets(true, cx))
            }))
            .child(pair(
                Field::new().label(tr("settings_name")).child(
                    Input::new(&self.name)
                        .prefix(self.identity(identity::Trigger::Inline, cx))
                        .disabled(locked)
                        .aria_label(tr("settings_name")),
                ),
                Field::new().label(tr("role_id")).child(
                    Input::new(&self.key)
                        .disabled(locked)
                        .aria_label(tr("role_id")),
                ),
            ))
            .child(
                Field::new().label(tr("teams_description")).child(
                    Input::new(&self.description)
                        .disabled(locked)
                        .aria_label(tr("teams_description")),
                ),
            )
            .child(
                Field::new()
                    .label(tr("teams_model"))
                    .child(self.model.clone()),
            )
            .child(self.read_only_row(cx))
            .child(self.prompt((!self.creating).then(|| self.presets(false, cx))))
            .into_any_element()
    }

    fn member(&self, cx: &mut Context<Self>) -> AnyElement {
        let locked = self.locked;
        v_flex()
            .w_full()
            .gap_4()
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .child(self.identity(identity::Trigger::Avatar, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                Input::new(&self.name)
                                    .appearance(false)
                                    .disabled(locked)
                                    .aria_label(tr("settings_name")),
                            )
                            .child(
                                Input::new(&self.description)
                                    .appearance(false)
                                    .small()
                                    .disabled(locked)
                                    .aria_label(tr("teams_description")),
                            ),
                    ),
            )
            .child(
                Field::new()
                    .label(tr("teams_model"))
                    .child(self.model.clone()),
            )
            .child(self.prompt(None))
            .child(self.read_only_row(cx))
            .into_any_element()
    }
}

impl Render for RoleForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.layout {
            Layout::Dialog => self.dialog(cx),
            Layout::Member => self.member(cx),
        }
    }
}
